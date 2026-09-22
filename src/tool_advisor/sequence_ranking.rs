//! Train and evaluate the experiment-only sequence-encoder rankers.
//!
//! The module is intentionally a research boundary.  It consumes the same
//! validated C001 cases as the existing advisor benchmarks, projects each
//! case through AdvisorContextV2, and writes self-describing local artifacts.
//! It never registers tools, makes permission decisions, or persists user
//! context.

use super::context_v2::AdvisorContextV2;
use super::sequence_encoder::{
    CandleBertSequenceEncoder, FineTuneStage, MarkerStrategy, PoolingStrategy,
};
use super::{
    baseline_prediction, dataset_fingerprint, evaluate, load_cases, partition_cases, MetricSummary,
    RankedCandidate, ToolAdvisorCase, ToolAdvisorPrediction, CASE_SCHEMA_VERSION, MAX_CANDIDATES,
};
use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, AdamW, Linear, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const RANKING_SCHEMA_VERSION: u16 = 1;
pub const RANKING_ARCHITECTURE_PAIRWISE: &str = "sequence-encoder-pairwise-v1";
pub const RANKING_ARCHITECTURE_PACKED: &str = "sequence-encoder-packed-marker-v1";
pub const RANKING_ARCHITECTURE_SHARED_PACKED: &str = "sequence-encoder-packed-shared-marker-v1";
pub const RANKING_ARCHITECTURE_SPAN_PACKED: &str = "sequence-encoder-packed-shared-span-v1";
pub const RANKING_ARCHITECTURE_BATCHED_PAIRWISE: &str = "sequence-encoder-batched-pairwise-v1";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RankingArchitecture {
    #[default]
    Pairwise,
    Packed,
    #[serde(rename = "shared-packed")]
    SharedPacked,
    #[serde(rename = "span-packed")]
    SpanPacked,
    #[serde(rename = "batched-pairwise")]
    BatchedPairwise,
}

impl RankingArchitecture {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pairwise => RANKING_ARCHITECTURE_PAIRWISE,
            Self::Packed => RANKING_ARCHITECTURE_PACKED,
            Self::SharedPacked => RANKING_ARCHITECTURE_SHARED_PACKED,
            Self::SpanPacked => RANKING_ARCHITECTURE_SPAN_PACKED,
            Self::BatchedPairwise => RANKING_ARCHITECTURE_BATCHED_PAIRWISE,
        }
    }

    /// (marker strategy, candidate representation, batching strategy)
    /// recorded in artifact manifests for M002+ architectures.
    pub fn contract(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Pairwise => (
                "distinct-ordinal-v1",
                "pair-cls-or-mean-v1",
                "per-candidate-forward-v1",
            ),
            Self::Packed => (
                "distinct-ordinal-v1",
                "marker-hidden-v1",
                "packed-single-forward-v1",
            ),
            Self::SharedPacked => (
                "shared-marker-v1",
                "marker-hidden-v1",
                "packed-single-forward-v1",
            ),
            Self::SpanPacked => (
                "shared-marker-v1",
                "descriptor-span-mean-v1",
                "packed-single-forward-v1",
            ),
            Self::BatchedPairwise => (
                "no-marker-pair-v1",
                "pair-cls-or-mean-v1",
                "batched-rows-single-forward-v1",
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRankingConfig {
    pub schema_version: u16,
    pub manifest_path: String,
    #[serde(default = "default_dataset")]
    pub dataset: String,
    pub output_artifact: String,
    #[serde(default)]
    pub architecture: RankingArchitecture,
    #[serde(default)]
    pub pooling: PoolingStrategy,
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,
    #[serde(default = "default_max_candidates")]
    pub max_candidates: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub seed: u64,
}

fn default_dataset() -> String {
    "assets/tool-advisor/corpus.jsonl".into()
}
fn default_stage() -> String {
    "head-only".into()
}
fn default_epochs() -> u32 {
    3
}
fn default_learning_rate() -> f64 {
    1e-3
}
fn default_max_candidates() -> usize {
    16
}
fn default_max_tokens() -> usize {
    256
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceArtifactManifest {
    pub artifact_schema_version: u16,
    pub architecture: String,
    pub framework: String,
    pub pooling: PoolingStrategy,
    pub fine_tuning_stage: String,
    pub encoder_manifest_path: String,
    pub encoder_config_sha256: String,
    pub tokenizer_sha256: String,
    pub source_weights_sha256: String,
    pub license_provenance_sha256: String,
    pub context_schema_version: u16,
    pub candidate_schema_version: u16,
    pub max_sequence_tokens: usize,
    pub max_candidates: usize,
    pub training_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub calibration: CalibrationParameters,
    pub final_weight_sha256: String,
    pub seed: u64,
    /// M002 architecture contract. Pre-contract v1 artifacts
    /// deserialize with the historical defaults below.
    #[serde(default = "default_manifest_marker_strategy")]
    pub marker_strategy: String,
    #[serde(default = "default_manifest_candidate_representation")]
    pub candidate_representation: String,
    #[serde(default = "default_manifest_batching_strategy")]
    pub batching_strategy: String,
    #[serde(default)]
    pub permutation_contract_version: u16,
    #[serde(default = "default_manifest_training_objective")]
    pub training_objective_version: String,
}

fn default_manifest_marker_strategy() -> String {
    "distinct-ordinal-v1".into()
}

fn default_manifest_candidate_representation() -> String {
    "marker-hidden-v1".into()
}

fn default_manifest_batching_strategy() -> String {
    "packed-single-forward-v1".into()
}

fn default_manifest_training_objective() -> String {
    "single-target-cross-entropy-v1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CalibrationParameters {
    pub method: String,
    pub abstention_threshold: f64,
    pub dev_brier: f64,
    pub dev_ece: f64,
    pub dev_nll: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRankingArtifact {
    pub manifest: SequenceArtifactManifest,
    pub head_weights_path: String,
    pub encoder_weights_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub brier: f64,
    pub ece: f64,
    pub nll: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingEvaluation {
    pub metrics: MetricSummary,
    pub calibration: CalibrationReport,
    pub dropped_candidates: usize,
    pub forwards: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageReport {
    pub stage: String,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub train_loss: f32,
    pub train: RankingEvaluation,
    pub dev: RankingEvaluation,
    pub artifact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRankingExperimentReport {
    pub schema_version: u16,
    pub architecture: String,
    pub pooling: PoolingStrategy,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub test_cases: usize,
    pub baseline_dev: MetricSummary,
    pub stages: Vec<StageReport>,
    pub selected_stage: Option<String>,
    pub selected_artifact: Option<String>,
    pub selected_dev: Option<RankingEvaluation>,
    pub local_asset: String,
    pub source_provenance: String,
}

pub struct SequenceRankingHead {
    pub varmap: VarMap,
    relevance: Linear,
    abstention: Linear,
}

impl SequenceRankingHead {
    pub fn new(hidden_size: usize, device: &Device) -> Result<Self> {
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, device);
        Ok(Self {
            relevance: linear(hidden_size, 1, vb.pp("relevance"))?,
            abstention: linear(hidden_size, 1, vb.pp("abstention"))?,
            varmap,
        })
    }

    fn score(&self, vector: &[f32], device: &Device) -> Result<Tensor> {
        let vector = Tensor::new(vector, device)?.unsqueeze(0)?;
        Ok(self.relevance.forward(&vector)?.squeeze(0)?.squeeze(0)?)
    }

    fn abstain(&self, vector: &[f32], device: &Device) -> Result<Tensor> {
        let vector = Tensor::new(vector, device)?.unsqueeze(0)?;
        Ok(self.abstention.forward(&vector)?.squeeze(0)?.squeeze(0)?)
    }

    fn scores(&self, vectors: &[Vec<f32>], device: &Device) -> Result<Tensor> {
        let values = vectors
            .iter()
            .map(|vector| self.score(vector, device))
            .collect::<Result<Vec<_>>>()?;
        Ok(Tensor::stack(&values, 0)?.unsqueeze(0)?)
    }
}

pub struct SequenceRanker {
    pub encoder: CandleBertSequenceEncoder,
    pub head: SequenceRankingHead,
    pub manifest: SequenceArtifactManifest,
    pub abstention_threshold: f64,
}

#[derive(Debug, Clone)]
struct CaseFeatures {
    candidate_names: Vec<String>,
    candidate_vectors: Vec<Vec<f32>>,
    context_vector: Vec<f32>,
    dropped_candidates: usize,
    forwards: usize,
}

fn descriptor(candidate: &super::ToolAdvisorCandidate) -> String {
    format!(
        "canonical name: {}; description: {}; category: {}; disclosure: {}",
        candidate.name, candidate.description, candidate.category, candidate.disclosure
    )
}

fn context_v2(case: &ToolAdvisorCase) -> String {
    AdvisorContextV2::from_benchmark_context(&case.context).serialize()
}

impl SequenceRanker {
    fn features(&self, case: &ToolAdvisorCase) -> Result<CaseFeatures> {
        let context = context_v2(case);
        let candidates = case
            .candidates
            .iter()
            .take(self.manifest.max_candidates)
            .collect::<Vec<_>>();
        let mut forwards = 1;
        let (candidate_names, candidate_vectors, dropped_candidates) =
            match self.manifest.architecture.as_str() {
                RANKING_ARCHITECTURE_PAIRWISE => {
                    let mut vectors = Vec::with_capacity(candidates.len());
                    for candidate in &candidates {
                        vectors.push(self.encoder.encode_with_pooling(
                            &context,
                            &descriptor(candidate),
                            self.manifest.pooling,
                        )?);
                        forwards += 1;
                    }
                    let names = candidates
                        .iter()
                        .map(|candidate| candidate.name.clone())
                        .collect();
                    (names, vectors, 0)
                }
                RANKING_ARCHITECTURE_PACKED => {
                    let names = candidates
                        .iter()
                        .map(|candidate| descriptor(candidate))
                        .collect::<Vec<_>>();
                    let packed = self.encoder.packed_encoding(
                        &context,
                        &names,
                        self.manifest.max_sequence_tokens,
                    )?;
                    let hidden = self.encoder.packed_hidden(&packed)?;
                    let mut vectors = Vec::with_capacity(packed.candidate_indices.len());
                    for position in &packed.marker_positions {
                        vectors.push(
                            hidden
                                .narrow(1, *position, 1)?
                                .squeeze(1)?
                                .squeeze(0)?
                                .to_dtype(DType::F32)?
                                .to_vec1()?,
                        );
                    }
                    forwards += 1;
                    // v1 truncates from the first overflowing candidate, so
                    // scored names are the presentation-order prefix.
                    let scored = candidates
                        .iter()
                        .take(vectors.len())
                        .map(|candidate| candidate.name.clone())
                        .collect();
                    (scored, vectors, packed.dropped_candidate_indices.len())
                }
                RANKING_ARCHITECTURE_SHARED_PACKED => {
                    let names = candidates
                        .iter()
                        .map(|candidate| descriptor(candidate))
                        .collect::<Vec<_>>();
                    let packed = self.encoder.packed_encoding_with_strategy(
                        &context,
                        &names,
                        self.manifest.max_sequence_tokens,
                        MarkerStrategy::Shared,
                    )?;
                    let hidden = self.encoder.packed_hidden(&packed)?;
                    let mut vectors = Vec::with_capacity(packed.candidate_indices.len());
                    for position in &packed.marker_positions {
                        vectors.push(
                            hidden
                                .narrow(1, *position, 1)?
                                .squeeze(1)?
                                .squeeze(0)?
                                .to_dtype(DType::F32)?
                                .to_vec1()?,
                        );
                    }
                    forwards += 1;
                    // Order-independent truncation can drop middle
                    // candidates, so names map through candidate_indices.
                    let scored = packed
                        .candidate_indices
                        .iter()
                        .map(|index| candidates[*index].name.clone())
                        .collect();
                    (scored, vectors, packed.dropped_candidate_indices.len())
                }
                RANKING_ARCHITECTURE_SPAN_PACKED => {
                    let names = candidates
                        .iter()
                        .map(|candidate| descriptor(candidate))
                        .collect::<Vec<_>>();
                    let packed = self.encoder.packed_encoding_with_strategy(
                        &context,
                        &names,
                        self.manifest.max_sequence_tokens,
                        MarkerStrategy::Shared,
                    )?;
                    let vectors = self.encoder.packed_span_vectors(&packed)?;
                    forwards += 1;
                    let scored = packed
                        .candidate_indices
                        .iter()
                        .map(|index| candidates[*index].name.clone())
                        .collect();
                    (scored, vectors, packed.dropped_candidate_indices.len())
                }
                RANKING_ARCHITECTURE_BATCHED_PAIRWISE => {
                    let names = candidates
                        .iter()
                        .map(|candidate| descriptor(candidate))
                        .collect::<Vec<_>>();
                    let batch =
                        self.encoder
                            .batch_encode_pairs(&context, &names, self.manifest.pooling)?;
                    forwards += batch.forwards;
                    let scored = candidates
                        .iter()
                        .map(|candidate| candidate.name.clone())
                        .collect();
                    (scored, batch.vectors, 0)
                }
                other => return Err(anyhow!("unsupported ranking architecture {other}")),
            };
        let context_vector = self
            .encoder
            .encode_context(&context, self.manifest.pooling)?;
        Ok(CaseFeatures {
            candidate_names,
            candidate_vectors,
            context_vector,
            dropped_candidates,
            forwards,
        })
    }

    pub fn predict_case(
        &self,
        case: &ToolAdvisorCase,
    ) -> Result<(ToolAdvisorPrediction, usize, usize)> {
        let features = self.features(case)?;
        self.predict_features(case, &features)
    }

    fn predict_features(
        &self,
        case: &ToolAdvisorCase,
        features: &CaseFeatures,
    ) -> Result<(ToolAdvisorPrediction, usize, usize)> {
        let logits = self
            .head
            .scores(&features.candidate_vectors, &self.encoder.device)?;
        let logits = logits.squeeze(0)?.to_vec1::<f32>()?;
        let abstain = candle_nn::ops::sigmoid(
            &self
                .head
                .abstain(&features.context_vector, &self.encoder.device)?,
        )?
        .to_vec0::<f32>()? as f64;
        let mut ranked = features
            .candidate_names
            .iter()
            .cloned()
            .zip(logits)
            .map(|(name, score)| RankedCandidate {
                name,
                score: score as f64,
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok((
            ToolAdvisorPrediction {
                schema_version: super::PREDICTION_SCHEMA_VERSION,
                case_id: case.case_id.clone(),
                ranked,
                abstain_probability: Some(abstain),
                mode: self.manifest.architecture.clone(),
            },
            features.dropped_candidates,
            features.forwards,
        ))
    }

    fn evaluate_features(
        &self,
        cases: &[ToolAdvisorCase],
        features: &[CaseFeatures],
    ) -> Result<RankingEvaluation> {
        let started = Instant::now();
        if cases.len() != features.len() {
            return Err(anyhow!("cached feature count does not match case count"));
        }
        let mut predictions = Vec::with_capacity(cases.len());
        let mut dropped = 0;
        let mut forwards = 0;
        for (case, features) in cases.iter().zip(features) {
            let (prediction, case_dropped, case_forwards) =
                self.predict_features(case, features)?;
            predictions.push(prediction);
            dropped += case_dropped;
            forwards += case_forwards;
        }
        let metrics = evaluate(cases, &predictions)?;
        let calibration = calibration_report(cases, &predictions)?;
        Ok(RankingEvaluation {
            metrics,
            calibration,
            dropped_candidates: dropped,
            forwards,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub fn evaluate_cases(&self, cases: &[ToolAdvisorCase]) -> Result<RankingEvaluation> {
        let started = Instant::now();
        let mut predictions = Vec::with_capacity(cases.len());
        let mut dropped = 0;
        let mut forwards = 0;
        for case in cases {
            let (prediction, case_dropped, case_forwards) = self.predict_case(case)?;
            predictions.push(prediction);
            dropped += case_dropped;
            forwards += case_forwards;
        }
        let metrics = evaluate(cases, &predictions)?;
        let calibration = calibration_report(cases, &predictions)?;
        Ok(RankingEvaluation {
            metrics,
            calibration,
            dropped_candidates: dropped,
            forwards,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

fn calibration_report(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
) -> Result<CalibrationReport> {
    let mut rows = Vec::new();
    for case in cases {
        let prediction = predictions
            .iter()
            .find(|prediction| prediction.case_id == case.case_id)
            .ok_or_else(|| anyhow!("missing prediction for {}", case.case_id))?;
        let probability = prediction.abstain_probability.unwrap_or(0.0);
        let target = if case.none { 1.0 } else { 0.0 };
        rows.push((probability, target));
    }
    if rows.is_empty() {
        return Err(anyhow!("calibration requires at least one case"));
    }
    let brier = rows
        .iter()
        .map(|(prediction, target)| (prediction - target).powi(2))
        .sum::<f64>()
        / rows.len() as f64;
    let nll = rows
        .iter()
        .map(|(prediction, target)| {
            let probability = prediction.clamp(1e-6, 1.0 - 1e-6);
            -(target * probability.ln() + (1.0 - target) * (1.0 - probability).ln())
        })
        .sum::<f64>()
        / rows.len() as f64;
    let mut sorted = rows.clone();
    sorted.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(Ordering::Equal));
    let bins = 10usize;
    let ece = (0..bins)
        .map(|bin| {
            let lower = bin as f64 / bins as f64;
            let upper = (bin + 1) as f64 / bins as f64;
            let values = sorted
                .iter()
                .filter(|(prediction, _)| {
                    (*prediction >= lower && *prediction < upper)
                        || (bin + 1 == bins && *prediction <= upper)
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                0.0
            } else {
                let predicted =
                    values.iter().map(|(value, _)| *value).sum::<f64>() / values.len() as f64;
                let actual =
                    values.iter().map(|(_, target)| *target).sum::<f64>() / values.len() as f64;
                (predicted - actual).abs() * values.len() as f64 / rows.len() as f64
            }
        })
        .sum();
    Ok(CalibrationReport { brier, ece, nll })
}

fn parse_stage(value: &str) -> Result<FineTuneStage> {
    match value {
        "head-only" => Ok(FineTuneStage::HeadOnly),
        "top-1" => Ok(FineTuneStage::TopLayers(1)),
        "top-2" => Ok(FineTuneStage::TopLayers(2)),
        "full" => Ok(FineTuneStage::Full),
        other => Err(anyhow!("unsupported sequence ranking stage {other}")),
    }
}

fn validate_config(config: &SequenceRankingConfig) -> Result<()> {
    if config.schema_version != RANKING_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported sequence ranking schema {}",
            config.schema_version
        ));
    }
    if config.epochs == 0
        || config.epochs > 100
        || !config.learning_rate.is_finite()
        || config.learning_rate <= 0.0
    {
        return Err(anyhow!(
            "invalid sequence ranking epoch or learning-rate bounds"
        ));
    }
    if config.max_candidates == 0
        || config.max_candidates > MAX_CANDIDATES
        || config.max_tokens < 8
        || config.max_tokens > 512
    {
        return Err(anyhow!("invalid sequence ranking input bounds"));
    }
    let _ = parse_stage(&config.stage)?;
    Ok(())
}

pub fn load_config(path: &Path) -> Result<SequenceRankingConfig> {
    let bytes = fs::read(path)
        .with_context(|| format!("read sequence ranking config {}", path.display()))?;
    let config = if path.extension().and_then(|extension| extension.to_str()) == Some("toml") {
        toml::from_str(
            std::str::from_utf8(&bytes).context("sequence ranking config is not UTF-8")?,
        )?
    } else {
        serde_json::from_slice(&bytes)?
    };
    validate_config(&config)?;
    Ok(config)
}

fn loss_for_case(
    head: &SequenceRankingHead,
    features: &CaseFeatures,
    case: &ToolAdvisorCase,
    device: &Device,
) -> Result<Tensor> {
    if features.candidate_vectors.is_empty() {
        return Err(anyhow!("case {} has no scoreable candidates", case.case_id));
    }
    let abstain_logit = head.abstain(&features.context_vector, device)?;
    let abstain_target = Tensor::new(&[if case.none { 1.0f32 } else { 0.0 }], device)?;
    let abstain_loss = candle_nn::loss::binary_cross_entropy_with_logit(
        &abstain_logit.unsqueeze(0)?,
        &abstain_target,
    )?;
    if case.none || case.relevance.is_empty() {
        return Ok(abstain_loss);
    }
    let target_index = features
        .candidate_names
        .iter()
        .enumerate()
        .max_by_key(|(_, name)| case.relevance.get(*name).copied().unwrap_or(0))
        .map(|(index, _)| index as u32)
        .ok_or_else(|| anyhow!("case {} has no target candidate", case.case_id))?;
    let logits = head.scores(&features.candidate_vectors, device)?;
    let target = Tensor::new(&[target_index], device)?;
    let ranking_loss = candle_nn::loss::cross_entropy(&logits, &target)?;
    Ok((ranking_loss + abstain_loss)?.affine(0.5, 0.0)?)
}

fn artifact_hash(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path).with_context(
        || format!("read artifact {}", path.display()),
    )?)))
}

fn partition_fingerprint(cases: &[ToolAdvisorCase], indices: &[usize]) -> Result<String> {
    dataset_fingerprint(
        &indices
            .iter()
            .map(|index| cases[*index].clone())
            .collect::<Vec<_>>(),
    )
}

fn save_artifact(
    ranker: &SequenceRanker,
    config: &SequenceRankingConfig,
    cases: &[ToolAdvisorCase],
    train_indices: &[usize],
    dev_indices: &[usize],
    output: &Path,
    stage: FineTuneStage,
    calibration: CalibrationParameters,
) -> Result<SequenceRankingArtifact> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let head_path = output.with_extension("head.safetensors");
    ranker.head.varmap.save(&head_path)?;
    let encoder_path = if !matches!(stage, FineTuneStage::HeadOnly) {
        let path = output.with_extension("encoder.safetensors");
        ranker.encoder.varmap.save(&path)?;
        Some(path.display().to_string())
    } else {
        None
    };
    let assets = &ranker.encoder.assets;
    let final_weight_sha256 = artifact_hash(&head_path)?;
    let (marker_strategy, candidate_representation, batching_strategy) =
        config.architecture.contract();
    let manifest = SequenceArtifactManifest {
        artifact_schema_version: RANKING_SCHEMA_VERSION,
        architecture: config.architecture.as_str().into(),
        framework: "candle-0.11".into(),
        pooling: config.pooling,
        fine_tuning_stage: config.stage.clone(),
        encoder_manifest_path: assets.manifest_path.display().to_string(),
        encoder_config_sha256: assets.manifest.hashes["config"].clone(),
        tokenizer_sha256: assets.manifest.hashes["vocabulary"].clone(),
        source_weights_sha256: assets.manifest.hashes["weights"].clone(),
        license_provenance_sha256: assets.manifest.hashes["license"].clone(),
        context_schema_version: super::context_v2::ADVISOR_CONTEXT_V2_SCHEMA_VERSION,
        candidate_schema_version: CASE_SCHEMA_VERSION,
        max_sequence_tokens: config.max_tokens,
        max_candidates: config.max_candidates,
        training_partition_fingerprint: partition_fingerprint(cases, train_indices)?,
        dev_partition_fingerprint: partition_fingerprint(cases, dev_indices)?,
        calibration,
        final_weight_sha256,
        seed: config.seed,
        marker_strategy: marker_strategy.into(),
        candidate_representation: candidate_representation.into(),
        batching_strategy: batching_strategy.into(),
        permutation_contract_version: super::order_invariance::PERMUTATION_CONTRACT_VERSION,
        training_objective_version: default_manifest_training_objective(),
    };
    let artifact = SequenceRankingArtifact {
        manifest,
        head_weights_path: head_path.display().to_string(),
        encoder_weights_path: encoder_path,
    };
    let bytes = serde_json::to_vec_pretty(&artifact)?;
    let temp = output.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, output)?;
    Ok(artifact)
}

pub fn load_artifact(path: &Path, device: &Device) -> Result<SequenceRanker> {
    let artifact: SequenceRankingArtifact = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read sequence artifact {}", path.display()))?,
    )
    .context("parse sequence ranking artifact")?;
    if artifact.manifest.artifact_schema_version != RANKING_SCHEMA_VERSION {
        return Err(anyhow!("unsupported sequence ranking artifact schema"));
    }
    let encoder = CandleBertSequenceEncoder::load(
        Path::new(&artifact.manifest.encoder_manifest_path),
        device,
    )?;
    let head = SequenceRankingHead::new(encoder.config.hidden_size, device)?;
    if artifact.manifest.final_weight_sha256
        != artifact_hash(Path::new(&artifact.head_weights_path))?
    {
        return Err(anyhow!("sequence ranking head hash mismatch"));
    }
    head.varmap
        .clone()
        .load(Path::new(&artifact.head_weights_path))?;
    if let Some(path) = &artifact.encoder_weights_path {
        encoder.varmap.clone().load(path)?;
    }
    Ok(SequenceRanker {
        abstention_threshold: artifact.manifest.calibration.abstention_threshold,
        encoder,
        head,
        manifest: artifact.manifest,
    })
}

/// Inference-only cost probe for one ranker architecture (M002 §7).
///
/// No training happens here: the probe encodes representative
/// candidate sets and reports forwards, tokens, wall latency, batch
/// tensor shape, and process RSS delta. Architecture selection in M003
/// owns quality; this probe only documents the cost envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureCostProbe {
    pub architecture: String,
    pub candidates: usize,
    pub forwards: usize,
    pub total_tokens: usize,
    pub batch_rows: usize,
    pub batch_cols: usize,
    pub samples: usize,
    pub mean_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub max_latency_ms: u128,
    pub rss_delta_bytes: Option<i64>,
}

#[cfg(target_os = "linux")]
fn process_rss_bytes() -> Option<i64> {
    // Resident set from procfs: no unsafe, no new dependency. VmRSS is
    // reported in kB.
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(kb) = line.strip_prefix("VmRSS:") {
            let kb: i64 = kb.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn process_rss_bytes() -> Option<i64> {
    // No portable unsafe-free RSS source on this target; the probe
    // records latency/tokens/forwards and leaves RSS empty.
    None
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (quantile * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

pub fn probe_architecture_cost(
    encoder: &CandleBertSequenceEncoder,
    architecture: RankingArchitecture,
    context: &str,
    descriptors: &[String],
    max_tokens: usize,
    iterations: usize,
) -> Result<ArchitectureCostProbe> {
    use super::sequence_encoder::MarkerStrategy;
    if descriptors.is_empty() || iterations == 0 {
        return Err(anyhow!("cost probe needs candidates and iterations"));
    }
    let rss_before = process_rss_bytes();
    let mut samples = Vec::with_capacity(iterations);
    let (mut forwards, mut total_tokens, mut batch_rows, mut batch_cols) = (0, 0, 0, 0);
    for _ in 0..iterations {
        let started = Instant::now();
        match architecture {
            RankingArchitecture::Pairwise | RankingArchitecture::BatchedPairwise => {
                if architecture == RankingArchitecture::BatchedPairwise {
                    let batch =
                        encoder.batch_encode_pairs(context, descriptors, PoolingStrategy::Mean)?;
                    forwards = batch.forwards;
                    total_tokens = batch.total_tokens;
                    batch_rows = batch.batch_rows;
                    batch_cols = batch.batch_cols;
                } else {
                    let mut tokens = 0;
                    for descriptor_text in descriptors {
                        let encoded = encoder.tokenizer.encode_pair(
                            context,
                            descriptor_text,
                            super::sequence_encoder::MAX_PAIR_TOKENS,
                        );
                        tokens += encoded.input_ids.len();
                        let _ = encoder.encode_with_pooling(
                            context,
                            descriptor_text,
                            PoolingStrategy::Mean,
                        )?;
                    }
                    forwards = descriptors.len();
                    total_tokens = tokens;
                    batch_rows = descriptors.len();
                    batch_cols = 0;
                }
            }
            RankingArchitecture::Packed => {
                let packed = encoder.packed_encoding(context, descriptors, max_tokens)?;
                total_tokens = packed.input_ids.len();
                let _ = encoder.packed_hidden(&packed)?;
                forwards = 1;
            }
            RankingArchitecture::SharedPacked | RankingArchitecture::SpanPacked => {
                let packed = encoder.packed_encoding_with_strategy(
                    context,
                    descriptors,
                    max_tokens,
                    MarkerStrategy::Shared,
                )?;
                total_tokens = packed.input_ids.len();
                if architecture == RankingArchitecture::SpanPacked {
                    let _ = encoder.packed_span_vectors(&packed)?;
                } else {
                    let _ = encoder.packed_hidden(&packed)?;
                }
                forwards = 1;
            }
        }
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let mean_latency_ms = samples.iter().sum::<f64>() / samples.len() as f64;
    let max_latency_ms = samples.last().copied().unwrap_or(0.0) as u128;
    let rss_delta_bytes = match (rss_before, process_rss_bytes()) {
        (Some(before), Some(after)) => Some(after - before),
        _ => None,
    };
    Ok(ArchitectureCostProbe {
        architecture: architecture.as_str().into(),
        candidates: descriptors.len(),
        forwards,
        total_tokens,
        batch_rows,
        batch_cols,
        samples: samples.len(),
        mean_latency_ms,
        p50_latency_ms: percentile(&samples, 0.50),
        p95_latency_ms: percentile(&samples, 0.95),
        max_latency_ms,
        rss_delta_bytes,
    })
}

pub fn train(
    config: &SequenceRankingConfig,
    device: &Device,
) -> Result<SequenceRankingExperimentReport> {
    validate_config(config)?;
    let cases = load_cases(Some(Path::new(&config.dataset)))?;
    let dataset_fingerprint = dataset_fingerprint(&cases)?;
    let partition = partition_cases(&cases);
    if partition.train_cases.is_empty() || partition.dev_cases.is_empty() {
        return Err(anyhow!(
            "sequence ranking requires non-empty train and dev partitions"
        ));
    }
    let train_cases = partition
        .train_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect::<Vec<_>>();
    let dev_cases = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect::<Vec<_>>();
    let baseline_dev = evaluate(
        &dev_cases,
        &dev_cases
            .iter()
            .map(|case| baseline_prediction(case, crate::tool::catalog::SearchMode::BM25))
            .collect::<Vec<_>>(),
    )?;
    device.set_seed(config.seed).ok();
    let encoder = CandleBertSequenceEncoder::load(Path::new(&config.manifest_path), device)?;
    let head = SequenceRankingHead::new(encoder.config.hidden_size, device)?;
    let stage = parse_stage(&config.stage)?;
    let ranker = SequenceRanker {
        manifest: placeholder_manifest(&encoder, config, stage),
        encoder,
        head,
        abstention_threshold: 0.5,
    };
    let mut optimizer_vars = ranker.head.varmap.all_vars();
    match stage {
        FineTuneStage::HeadOnly => {}
        FineTuneStage::TopLayers(count) => {
            optimizer_vars.extend(ranker.encoder.top_layer_vars(count.max(1)))
        }
        FineTuneStage::Full => optimizer_vars.extend(ranker.encoder.varmap.all_vars()),
    }
    let mut optimizer = AdamW::new(
        optimizer_vars,
        ParamsAdamW {
            lr: config.learning_rate,
            weight_decay: 0.0,
            ..Default::default()
        },
    )?;
    let cached_train = matches!(stage, FineTuneStage::HeadOnly)
        .then(|| {
            train_cases
                .iter()
                .map(|case| ranker.features(case))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let cached_dev = matches!(stage, FineTuneStage::HeadOnly)
        .then(|| {
            dev_cases
                .iter()
                .map(|case| ranker.features(case))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let mut last_loss = 0.0;
    for _epoch in 0..config.epochs {
        for (index, case) in train_cases.iter().enumerate() {
            let features = match cached_train.as_ref() {
                Some(features) => features[index].clone(),
                None => ranker.features(case)?,
            };
            let loss = loss_for_case(&ranker.head, &features, case, device)?;
            last_loss = loss.to_vec0::<f32>()?;
            optimizer.backward_step(&loss)?;
        }
    }
    let train_eval = match cached_train.as_ref() {
        Some(features) => ranker.evaluate_features(&train_cases, features)?,
        None => ranker.evaluate_cases(&train_cases)?,
    };
    let dev_eval = match cached_dev.as_ref() {
        Some(features) => ranker.evaluate_features(&dev_cases, features)?,
        None => ranker.evaluate_cases(&dev_cases)?,
    };
    let calibration = CalibrationParameters {
        method: "dev-abstention-threshold-v1".into(),
        abstention_threshold: best_abstention_threshold(
            &dev_cases,
            &ranker,
            cached_dev.as_deref(),
        )?,
        dev_brier: dev_eval.calibration.brier,
        dev_ece: dev_eval.calibration.ece,
        dev_nll: dev_eval.calibration.nll,
    };
    let output = PathBuf::from(&config.output_artifact);
    let _artifact = save_artifact(
        &ranker,
        config,
        &cases,
        &partition.train_cases,
        &partition.dev_cases,
        &output,
        stage,
        calibration,
    )?;
    let stage_report = StageReport {
        stage: config.stage.clone(),
        train_cases: train_cases.len(),
        dev_cases: dev_cases.len(),
        train_loss: last_loss,
        train: train_eval,
        dev: dev_eval.clone(),
        artifact: Some(output.display().to_string()),
    };
    Ok(SequenceRankingExperimentReport {
        schema_version: RANKING_SCHEMA_VERSION,
        architecture: config.architecture.as_str().into(),
        pooling: config.pooling,
        dataset_fingerprint,
        train_partition_fingerprint: partition_fingerprint(&cases, &partition.train_cases)?,
        dev_partition_fingerprint: partition_fingerprint(&cases, &partition.dev_cases)?,
        train_cases: train_cases.len(),
        dev_cases: dev_cases.len(),
        test_cases: partition.test_cases.len(),
        baseline_dev,
        stages: vec![stage_report],
        selected_stage: Some(config.stage.clone()),
        selected_artifact: Some(output.display().to_string()),
        selected_dev: Some(dev_eval),
        local_asset: config.manifest_path.clone(),
        source_provenance: format!(
            "{}@{}",
            ranker.encoder.assets.manifest.architecture, ranker.encoder.assets.manifest.framework
        ),
    })
}

fn placeholder_manifest(
    encoder: &CandleBertSequenceEncoder,
    config: &SequenceRankingConfig,
    stage: FineTuneStage,
) -> SequenceArtifactManifest {
    let (marker_strategy, candidate_representation, batching_strategy) =
        config.architecture.contract();
    SequenceArtifactManifest {
        artifact_schema_version: RANKING_SCHEMA_VERSION,
        architecture: config.architecture.as_str().into(),
        framework: "candle-0.11".into(),
        pooling: config.pooling,
        fine_tuning_stage: match stage {
            FineTuneStage::HeadOnly => "head-only",
            FineTuneStage::TopLayers(1) => "top-1",
            FineTuneStage::TopLayers(2) => "top-2",
            FineTuneStage::TopLayers(_) => "top-layers",
            FineTuneStage::Full => "full",
        }
        .into(),
        encoder_manifest_path: encoder.assets.manifest_path.display().to_string(),
        encoder_config_sha256: encoder.assets.manifest.hashes["config"].clone(),
        tokenizer_sha256: encoder.assets.manifest.hashes["vocabulary"].clone(),
        source_weights_sha256: encoder.assets.manifest.hashes["weights"].clone(),
        license_provenance_sha256: encoder.assets.manifest.hashes["license"].clone(),
        context_schema_version: super::context_v2::ADVISOR_CONTEXT_V2_SCHEMA_VERSION,
        candidate_schema_version: CASE_SCHEMA_VERSION,
        max_sequence_tokens: config.max_tokens,
        max_candidates: config.max_candidates,
        training_partition_fingerprint: String::new(),
        dev_partition_fingerprint: String::new(),
        calibration: CalibrationParameters {
            method: "dev-abstention-threshold-v1".into(),
            abstention_threshold: 0.5,
            ..CalibrationParameters::default()
        },
        final_weight_sha256: String::new(),
        seed: config.seed,
        marker_strategy: marker_strategy.into(),
        candidate_representation: candidate_representation.into(),
        batching_strategy: batching_strategy.into(),
        permutation_contract_version: super::order_invariance::PERMUTATION_CONTRACT_VERSION,
        training_objective_version: default_manifest_training_objective(),
    }
}

fn best_abstention_threshold(
    cases: &[ToolAdvisorCase],
    ranker: &SequenceRanker,
    cached_features: Option<&[CaseFeatures]>,
) -> Result<f64> {
    let mut candidates = vec![0.5];
    for (index, case) in cases.iter().enumerate() {
        let (prediction, _, _) = match cached_features {
            Some(features) => ranker.predict_features(case, &features[index])?,
            None => ranker.predict_case(case)?,
        };
        if let Some(value) = prediction.abstain_probability {
            candidates.push(value);
        }
    }
    let mut best = (f64::MIN, 0.5);
    for threshold in candidates {
        let mut tp = 0.0;
        let mut predicted = 0.0;
        let mut actual = 0.0;
        for (index, case) in cases.iter().enumerate() {
            let (prediction, _, _) = match cached_features {
                Some(features) => ranker.predict_features(case, &features[index])?,
                None => ranker.predict_case(case)?,
            };
            let value = prediction.abstain_probability.unwrap_or(0.0) >= threshold;
            if value {
                predicted += 1.0;
            }
            if case.none {
                actual += 1.0;
            }
            if value && case.none {
                tp += 1.0;
            }
        }
        let precision = if predicted > 0.0 { tp / predicted } else { 0.0 };
        let recall = if actual > 0.0 { tp / actual } else { 0.0 };
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };
        if f1 > best.0 {
            best = (f1, threshold);
        }
    }
    Ok(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn calibration_is_finite_and_bounded() {
        let cases = vec![ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: "case".into(),
            context: "read a file".into(),
            candidates: vec![],
            relevance: BTreeMap::new(),
            preferred_order: vec![],
            none: true,
            tags: vec![],
            group_id: "group".into(),
            provenance: "test".into(),
            semantic_group: "group".into(),
            leakage_group: String::new(),
            task_family: "filesystem".into(),
            tool_family: "filesystem".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }];
        let predictions = vec![ToolAdvisorPrediction {
            schema_version: 1,
            case_id: "case".into(),
            ranked: vec![],
            abstain_probability: Some(0.8),
            mode: "test".into(),
        }];
        let report = calibration_report(&cases, &predictions).unwrap();
        assert!(report.brier.is_finite());
        assert!((0.0..=1.0).contains(&report.ece));
        assert!(report.nll.is_finite());
    }

    #[test]
    fn stage_parser_rejects_implicit_full_training() {
        assert_eq!(parse_stage("head-only").unwrap(), FineTuneStage::HeadOnly);
        assert!(parse_stage("all").is_err());
    }

    /// M002: new architecture ids are distinct from v1 and carry an
    /// explicit marker/representation/batching contract.
    #[test]
    fn new_architectures_are_versioned_distinctly() {
        let ids = [
            RankingArchitecture::Pairwise.as_str(),
            RankingArchitecture::Packed.as_str(),
            RankingArchitecture::SharedPacked.as_str(),
            RankingArchitecture::SpanPacked.as_str(),
            RankingArchitecture::BatchedPairwise.as_str(),
        ];
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 5, "architecture ids must be distinct");
        assert_eq!(
            RankingArchitecture::SharedPacked.as_str(),
            RANKING_ARCHITECTURE_SHARED_PACKED
        );
        assert_eq!(
            RankingArchitecture::SpanPacked.as_str(),
            RANKING_ARCHITECTURE_SPAN_PACKED
        );
        assert_eq!(
            RankingArchitecture::BatchedPairwise.as_str(),
            RANKING_ARCHITECTURE_BATCHED_PAIRWISE
        );
        // The v1 control keeps its historical contract; every new arm
        // differs in at least one contract field.
        let v1 = RankingArchitecture::Packed.contract();
        for arch in [
            RankingArchitecture::SharedPacked,
            RankingArchitecture::SpanPacked,
            RankingArchitecture::BatchedPairwise,
        ] {
            assert_ne!(arch.contract(), v1, "{arch:?} must differ from v1");
        }
        // Serde names: legacy arms keep theirs; new arms are explicit.
        let encoded = serde_json::to_string(&RankingArchitecture::SharedPacked).unwrap();
        assert_eq!(encoded, "\"shared-packed\"");
        let decoded: RankingArchitecture = serde_json::from_str("\"batched-pairwise\"").unwrap();
        assert_eq!(decoded, RankingArchitecture::BatchedPairwise);
        let legacy: RankingArchitecture = serde_json::from_str("\"packed\"").unwrap();
        assert_eq!(legacy, RankingArchitecture::Packed);
    }

    /// M002: historical packed v1 artifacts keep loading unchanged and
    /// deserialize with pre-contract defaults.
    #[test]
    fn historical_packed_v1_manifest_still_loads() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("target/tool-advisor/sequence-ranking-minilm-packed-head.json");
        if !path.exists() {
            eprintln!("SKIP: packed v1 artifact absent in this environment");
            return;
        }
        let bytes = std::fs::read(&path).expect("read v1 artifact");
        let artifact: SequenceRankingArtifact =
            serde_json::from_slice(&bytes).expect("parse v1 artifact");
        assert_eq!(artifact.manifest.architecture, RANKING_ARCHITECTURE_PACKED);
        assert_eq!(artifact.manifest.marker_strategy, "distinct-ordinal-v1");
        assert_eq!(artifact.manifest.permutation_contract_version, 0);
        assert_eq!(
            artifact.manifest.training_objective_version,
            "single-target-cross-entropy-v1"
        );
    }

    /// M002: batched pairwise mapped-back scores are invariant under
    /// candidate permutation within floating tolerance (real encoder;
    /// skipped where reference assets are absent).
    #[test]
    fn batched_pairwise_scores_are_invariant_under_permutation() {
        use super::super::order_invariance::{
            evaluate_permutation_robustness, generate_permutations, permute_case, DEV_SUITE_SEED,
        };
        use super::super::sequence_encoder::CandleBertSequenceEncoder;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest =
            root.join("target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json");
        if !manifest.exists() {
            eprintln!("SKIP: reference encoder assets absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let encoder = CandleBertSequenceEncoder::load(&manifest, &device).expect("load encoder");
        // Identity-mapped vectors: score rows once per permutation,
        // map back by descriptor, compare.
        let descriptors = vec![
            "canonical name: read; description: Read files".to_string(),
            "canonical name: write; description: Write files".to_string(),
            "canonical name: shell; description: Run shell commands".to_string(),
            "canonical name: grep; description: Search file contents".to_string(),
            "canonical name: git; description: Inspect git history".to_string(),
        ];
        let context = "AdvisorContextV2 find files that mention TODO";
        let canonical = encoder
            .batch_encode_pairs(context, &descriptors, PoolingStrategy::Mean)
            .expect("canonical batch");
        assert_eq!(canonical.forwards, 1);
        let mut worst_drift = 0.0f32;
        for permutation in generate_permutations(descriptors.len(), DEV_SUITE_SEED, 8)
            .iter()
            .skip(1)
        {
            let permuted: Vec<String> = permutation
                .iter()
                .map(|i| descriptors[*i].clone())
                .collect();
            let scored = encoder
                .batch_encode_pairs(context, &permuted, PoolingStrategy::Mean)
                .expect("permuted batch");
            // Map rows back to canonical descriptor identity.
            for (canonical_index, descriptor) in descriptors.iter().enumerate() {
                let permuted_row = permuted.iter().position(|d| d == descriptor).expect("map");
                let left = &canonical.vectors[canonical_index];
                let right = &scored.vectors[permuted_row];
                for (a, b) in left.iter().zip(right.iter()) {
                    worst_drift = worst_drift.max((a - b).abs());
                }
            }
        }
        assert!(
            worst_drift <= 1e-5,
            "batched pairwise drift {worst_drift:e} exceeds numeric tolerance"
        );
        // The M001 contract agrees: build a tiny advisor over the batch
        // encoder and require effectively exact consistency.
        use super::super::ToolAdvisor;
        struct BatchAdvisor<'a> {
            encoder: &'a CandleBertSequenceEncoder,
        }
        impl ToolAdvisor for BatchAdvisor<'_> {
            fn score(
                &self,
                input: &super::super::ToolAdvisorInput,
            ) -> anyhow::Result<ToolAdvisorPrediction> {
                let texts: Vec<String> = input
                    .candidates
                    .iter()
                    .map(|c| format!("canonical name: {}; description: {}", c.name, c.description))
                    .collect();
                let batch = self.encoder.batch_encode_pairs(
                    &input.context,
                    &texts,
                    PoolingStrategy::Mean,
                )?;
                // Fixed random-projection-free score: L2 norm orders
                // candidates deterministically by descriptor content.
                let ranked = input
                    .candidates
                    .iter()
                    .zip(batch.vectors.iter())
                    .map(|(candidate, vector)| RankedCandidate {
                        name: candidate.name.clone(),
                        score: vector
                            .iter()
                            .map(|v| f64::from(*v).powi(2))
                            .sum::<f64>()
                            .sqrt(),
                    })
                    .collect();
                Ok(ToolAdvisorPrediction {
                    schema_version: super::super::PREDICTION_SCHEMA_VERSION,
                    case_id: input.case_id.clone(),
                    ranked,
                    abstain_probability: Some(0.0),
                    mode: "batch-probe".into(),
                })
            }
        }
        let case = super::super::ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: "batch-invariance".into(),
            context: context.into(),
            candidates: descriptors
                .iter()
                .enumerate()
                .map(|(i, d)| super::super::ToolAdvisorCandidate {
                    name: format!("tool-{i}"),
                    description: d.clone(),
                    category: String::new(),
                    disclosure: String::new(),
                    synthetic_identity: false,
                })
                .collect(),
            relevance: BTreeMap::from([("tool-0".to_string(), 3)]),
            preferred_order: vec!["tool-0".into()],
            none: false,
            tags: Vec::new(),
            group_id: "batch-group".into(),
            provenance: "m002".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        };
        let advisor = BatchAdvisor { encoder: &encoder };
        let metrics =
            evaluate_permutation_robustness(&case, &advisor, DEV_SUITE_SEED, 8).expect("metrics");
        assert!(
            metrics.top1_identity_consistency >= 0.99,
            "pairwise reference must be effectively exact: {}",
            metrics.top1_identity_consistency
        );
        assert!(
            metrics.max_score_drift <= 1e-5,
            "pairwise score drift {} exceeds tolerance",
            metrics.max_score_drift
        );
        // Permuting the helper case must round-trip (contract sanity).
        let permutation = vec![4, 3, 2, 1, 0];
        let permuted = permute_case(&case, &permutation).expect("permute");
        assert_eq!(permuted.candidates[0].name, "tool-4");
    }

    /// M002: packed truncation around the token budget reports drops
    /// deterministically instead of silently changing scoreability.
    #[test]
    fn packed_budget_drops_are_reported_and_deterministic() {
        use super::super::order_invariance::generate_permutations;
        use super::super::sequence_encoder::{CandleBertSequenceEncoder, MarkerStrategy};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest =
            root.join("target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json");
        if !manifest.exists() {
            eprintln!("SKIP: reference encoder assets absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let encoder = CandleBertSequenceEncoder::load(&manifest, &device).expect("load encoder");
        let descriptors = vec![
            "canonical name: read; description: Read repository files from disk".to_string(),
            "canonical name: shell; description: Execute shell commands with Sandboxing and many extra words to be long".to_string(),
            "canonical name: grep; description: Search".to_string(),
            "canonical name: git; description: Show git log with patches and stats for review".to_string(),
        ];
        let tiny_budget = 48;
        let first = encoder
            .packed_encoding_with_strategy(
                "find files",
                &descriptors,
                tiny_budget,
                MarkerStrategy::Shared,
            )
            .expect("encoding");
        assert!(
            !first.dropped_candidate_indices.is_empty(),
            "tiny budget must force drops"
        );
        // Every permutation of the same descriptor multiset drops the
        // same descriptors.
        let surviving: std::collections::BTreeSet<&str> = first
            .candidate_indices
            .iter()
            .map(|i| descriptors[*i].as_str())
            .collect();
        for permutation in generate_permutations(descriptors.len(), 7, 6)
            .iter()
            .skip(1)
        {
            let permuted: Vec<String> = permutation
                .iter()
                .map(|i| descriptors[*i].clone())
                .collect();
            let encoding = encoder
                .packed_encoding_with_strategy(
                    "find files",
                    &permuted,
                    tiny_budget,
                    MarkerStrategy::Shared,
                )
                .expect("encoding");
            let resettled: std::collections::BTreeSet<&str> = encoding
                .candidate_indices
                .iter()
                .map(|i| permuted[*i].as_str())
                .collect();
            assert_eq!(surviving, resettled, "drops must be order-independent");
        }
    }

    /// M002 §7: inference-only cost probe structure on 4/8/16-candidate
    /// sets. Full wall-clock evidence is gathered once in the ignored
    /// receipt test; this asserts the probe contract stays total.
    #[test]
    #[ignore]
    fn architecture_cost_probe_receipt() {
        use super::super::sequence_encoder::CandleBertSequenceEncoder;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest =
            root.join("target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json");
        if !manifest.exists() {
            eprintln!("SKIP: reference encoder assets absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let encoder = CandleBertSequenceEncoder::load(&manifest, &device).expect("load encoder");
        let context = "AdvisorContextV2 locate repository files containing TODO markers";
        let mut receipts = Vec::new();
        for size in [4usize, 8, 16] {
            let descriptors: Vec<String> = (0..size)
                .map(|i| format!("canonical name: tool-{i}; description: synthetic descriptor number {i} for cost probing with padding variation"))
                .collect();
            for architecture in [
                RankingArchitecture::Packed,
                RankingArchitecture::SharedPacked,
                RankingArchitecture::SpanPacked,
                RankingArchitecture::BatchedPairwise,
            ] {
                let probe =
                    probe_architecture_cost(&encoder, architecture, context, &descriptors, 256, 5)
                        .expect("probe");
                assert_eq!(probe.candidates, size);
                assert!(probe.forwards >= 1);
                assert!(probe.total_tokens > 0);
                assert_eq!(probe.samples, 5);
                eprintln!(
                    "{}x{}: forwards={} tokens={} mean={:.1}ms p50={:.1}ms p95={:.1}ms max={}ms rss_delta={:?}",
                    architecture.as_str(),
                    size,
                    probe.forwards,
                    probe.total_tokens,
                    probe.mean_latency_ms,
                    probe.p50_latency_ms,
                    probe.p95_latency_ms,
                    probe.max_latency_ms,
                    probe.rss_delta_bytes,
                );
                receipts.push(probe);
            }
        }
        let out_dir = root.join("target/tool-advisor/order-invariance");
        std::fs::create_dir_all(&out_dir).expect("output dir");
        std::fs::write(
            out_dir.join("m002-cost-probe.json"),
            serde_json::to_vec_pretty(&receipts).expect("serialize"),
        )
        .expect("write receipt");
    }
}
