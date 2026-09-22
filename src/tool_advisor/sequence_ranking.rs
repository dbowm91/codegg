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
    RankedCandidate, ToolAdvisor, ToolAdvisorCase, ToolAdvisorInput, ToolAdvisorPrediction,
    CASE_SCHEMA_VERSION, MAX_CANDIDATES,
};
use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, AdamW, Linear, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
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
    /// M003 training objective weights. Absent fields default to the
    /// corrected graded objective, never to the legacy single-target
    /// loss: the correction is the point of M003.
    #[serde(default)]
    pub objective: ObjectiveWeights,
    /// M003 training input. `Canonical` preserves pre-M003 behavior;
    /// `BalancedPermutation` consumes the M001 balanced views.
    #[serde(default)]
    pub training_views: TrainingViewMode,
    /// Seed for balanced-view construction and consistency partners.
    #[serde(default = "default_permutation_seed")]
    pub permutation_seed: u64,
}

/// M003 corrected training objective weights (all >= 0).
///
/// - `listwise`: graded-relevance softmax cross-entropy over the
///   relevance head (grades 1..3 as `2^grade - 1` target mass).
/// - `binary_relevance`: per-candidate BCE on the same relevance
///   logits (positive iff relevance > 0, grade-weighted); the signal
///   M004 promotion calibration consumes.
/// - `abstention`: separate BCE on the context head vs `case.none`.
/// - `consistency`: permutation-consistency MSE between softmaxed
///   mapped-back relevance distributions of two presentations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ObjectiveWeights {
    #[serde(default = "default_weight_one")]
    pub listwise: f64,
    #[serde(default = "default_weight_one")]
    pub binary_relevance: f64,
    #[serde(default = "default_weight_one")]
    pub abstention: f64,
    #[serde(default)]
    pub consistency: f64,
}

impl Default for ObjectiveWeights {
    fn default() -> Self {
        Self {
            listwise: 1.0,
            binary_relevance: 1.0,
            abstention: 1.0,
            consistency: 0.0,
        }
    }
}

fn default_weight_one() -> f64 {
    1.0
}

fn default_permutation_seed() -> u64 {
    super::order_invariance::TRAIN_VIEW_SEED
}

/// M003 training input selector.
///
/// Defaults to `Canonical` so pre-M003 training configs keep their
/// exact behavior; M003 sweep configs set `BalancedPermutation`
/// explicitly.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TrainingViewMode {
    #[default]
    Canonical,
    BalancedPermutation,
}

/// Objective contract recorded in manifests.
pub const TRAINING_OBJECTIVE_GRADED_V1: &str = "graded-listwise-binary-v1";

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

/// Normalized graded-relevance target distribution over `names`.
///
/// Gains are `2^grade - 1` per labeled candidate; unlabeled candidates
/// get zero mass. Equal grades share equal mass, so two equally
/// relevant candidates are never collapsed to one target index.
pub fn graded_target_distribution(case: &ToolAdvisorCase, names: &[String]) -> Vec<f32> {
    let gains: Vec<f32> = names
        .iter()
        .map(|name| {
            case.relevance
                .get(name)
                .map(|grade| (1u32 << (*grade).min(3)) as f32 - 1.0)
                .unwrap_or(0.0)
        })
        .collect();
    let total: f32 = gains.iter().sum();
    if total <= 0.0 {
        return vec![0.0; names.len()];
    }
    gains.into_iter().map(|gain| gain / total).collect()
}

/// Binary relevance targets and grade weights parallel to `names`.
///
/// Positive iff relevance > 0; positives carry their grade (1..3) as
/// the sample weight, negatives carry 1.0.
pub fn binary_relevance_targets(case: &ToolAdvisorCase, names: &[String]) -> (Vec<f32>, Vec<f32>) {
    let mut targets = Vec::with_capacity(names.len());
    let mut weights = Vec::with_capacity(names.len());
    for name in names {
        match case.relevance.get(name) {
            Some(grade) => {
                targets.push(1.0);
                weights.push(f32::from(*grade));
            }
            None => {
                targets.push(0.0);
                weights.push(1.0);
            }
        }
    }
    (targets, weights)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CalibrationParameters {
    pub method: String,
    pub abstention_threshold: f64,
    pub dev_brier: f64,
    pub dev_ece: f64,
    pub dev_nll: f64,
    /// M003 candidate-relevance calibration: probability =
    /// sigmoid((logit + bias) / temperature). Pre-M003 artifacts
    /// deserialize with the identity (1.0, 0.0).
    #[serde(default = "default_candidate_temperature")]
    pub candidate_temperature: f64,
    #[serde(default)]
    pub candidate_bias: f64,
    /// Dev binary-relevance NLL before/after the temperature+bias fit.
    #[serde(default)]
    pub candidate_nll_before: f64,
    #[serde(default)]
    pub candidate_nll_after: f64,
}

fn default_candidate_temperature() -> f64 {
    1.0
}

/// Calibrated candidate-relevance probability for promotion use
/// (M004). Ranking order is unaffected (monotonic transform).
pub fn calibrated_relevance_probability(logit: f64, temperature: f64, bias: f64) -> f64 {
    let temperature = temperature.max(1e-3);
    let value = ((logit + bias) / temperature) as f32;
    (1.0 / (1.0 + (-value).exp())) as f64
}

fn binary_nll(logits: &[f64], labels: &[f64], temperature: f64, bias: f64) -> f64 {
    if logits.is_empty() {
        return f64::INFINITY;
    }
    let total: f64 = logits
        .iter()
        .zip(labels.iter())
        .map(|(logit, label)| {
            let probability =
                calibrated_relevance_probability(*logit, temperature, bias).clamp(1e-6, 1.0 - 1e-6);
            -(label * probability.ln() + (1.0 - label) * (1.0 - probability).ln())
        })
        .sum();
    total / logits.len() as f64
}

#[derive(Debug, Clone)]
pub struct CandidateCalibrationFit {
    pub temperature: f64,
    pub bias: f64,
    pub nll_before: f64,
    pub nll_after: f64,
}

/// Predeclared grid fit (`temperature-bias-grid-v1`): temperatures
/// {0.25, 0.5, 1.0, 2.0, 4.0} x biases {-2, -1, 0, 1, 2} minimizing dev
/// binary-relevance NLL. Identity (1.0, 0.0) is always a candidate, so
/// the fit can never be worse than uncalibrated.
pub fn fit_candidate_calibration(logits: &[f64], labels: &[f64]) -> CandidateCalibrationFit {
    let nll_before = binary_nll(logits, labels, 1.0, 0.0);
    let mut best = (1.0, 0.0, nll_before);
    for temperature in [0.25, 0.5, 1.0, 2.0, 4.0] {
        for bias in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            let nll = binary_nll(logits, labels, temperature, bias);
            if nll < best.2 {
                best = (temperature, bias, nll);
            }
        }
    }
    CandidateCalibrationFit {
        temperature: best.0,
        bias: best.1,
        nll_before,
        nll_after: best.2,
    }
}

/// Per-candidate relevance logits and binary labels over dev cases.
///
/// Logits are raw relevance-head outputs in case candidate order;
/// labels are 1 iff relevance > 0 (no-tool cases contribute all-zero
/// labels as hard negatives).
pub fn dev_relevance_observations(
    ranker: &SequenceRanker,
    cases: &[ToolAdvisorCase],
) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut logits = Vec::new();
    let mut labels = Vec::new();
    for case in cases {
        let (prediction, _, _) = ranker.predict_case(case)?;
        let by_name: std::collections::BTreeMap<&str, f64> = prediction
            .ranked
            .iter()
            .map(|item| (item.name.as_str(), item.score))
            .collect();
        for candidate in &case.candidates {
            if candidate.name.is_empty() {
                continue;
            }
            let Some(score) = by_name.get(candidate.name.as_str()).copied() else {
                continue;
            };
            // Dropped (unscoreable) candidates carry no relevance logit
            // and are excluded from calibration, never zero-filled.
            logits.push(score);
            labels.push(if case.relevance.contains_key(&candidate.name) {
                1.0
            } else {
                0.0
            });
        }
    }
    if logits.is_empty() {
        return Err(anyhow!("no relevance observations on dev cases"));
    }
    Ok((logits, labels))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryCalibrationReport {
    pub brier: f64,
    pub ece: f64,
    pub nll: f64,
    pub observations: usize,
}

/// Brier/ECE/NLL of calibrated candidate-relevance probabilities.
pub fn binary_calibration_report(
    logits: &[f64],
    labels: &[f64],
    temperature: f64,
    bias: f64,
) -> BinaryCalibrationReport {
    let probs: Vec<f64> = logits
        .iter()
        .map(|logit| calibrated_relevance_probability(*logit, temperature, bias))
        .collect();
    let brier = probs
        .iter()
        .zip(labels.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>()
        / probs.len() as f64;
    let mut sorted: Vec<(f64, f64)> = probs.into_iter().zip(labels.iter().copied()).collect();
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
                (predicted - actual).abs() * values.len() as f64 / sorted.len() as f64
            }
        })
        .sum();
    BinaryCalibrationReport {
        brier,
        ece,
        nll: binary_nll(logits, labels, temperature, bias),
        observations: sorted.len(),
    }
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
    /// M003 training input description. Pre-M003 reports deserialize
    /// with the canonical defaults.
    #[serde(default = "default_training_input_mode")]
    pub training_input_mode: String,
    #[serde(default)]
    pub training_input_fingerprint: String,
    #[serde(default = "default_report_objective_version")]
    pub training_objective_version: String,
}

fn default_training_input_mode() -> String {
    "canonical".into()
}

fn default_report_objective_version() -> String {
    "single-target-cross-entropy-v1".into()
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
    for (name, weight) in [
        ("listwise", config.objective.listwise),
        ("binary_relevance", config.objective.binary_relevance),
        ("abstention", config.objective.abstention),
        ("consistency", config.objective.consistency),
    ] {
        if !weight.is_finite() || weight < 0.0 {
            return Err(anyhow!("invalid training objective weight {name}"));
        }
    }
    if config.objective.listwise + config.objective.binary_relevance + config.objective.abstention
        <= 0.0
    {
        return Err(anyhow!(
            "training objective needs an active listwise, binary, or abstention term"
        ));
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

/// M003 corrected training loss: graded listwise relevance plus
/// per-candidate binary relevance on the relevance head, separate
/// abstention BCE on the context head, and an optional permutation-
/// consistency term over mapped-back relevance distributions.
///
/// `consistency_partner` carries the same source case under a second
/// deterministic presentation with per-name aligned logits; when
/// present and `weights.consistency > 0`, the MSE between the two
/// softmaxed relevance distributions is penalized. Loss terms are
/// normalized by the sum of active weights so learning rates stay
/// comparable across objective mixes.
#[allow(clippy::too_many_arguments)]
fn loss_for_case(
    head: &SequenceRankingHead,
    features: &CaseFeatures,
    case: &ToolAdvisorCase,
    device: &Device,
    weights: &ObjectiveWeights,
    consistency_partner: Option<&Tensor>,
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
    let mut total = abstain_loss.affine(weights.abstention.max(0.0), 0.0)?;
    let mut active_weight = weights.abstention.max(0.0);
    if !case.none && !case.relevance.is_empty() {
        let logits = head.scores(&features.candidate_vectors, device)?;
        // Listwise graded relevance: softmax CE against the normalized
        // gain distribution (no-tool cases omit this term).
        if weights.listwise > 0.0 {
            let targets = graded_target_distribution(case, &features.candidate_names);
            let distribution = Tensor::new(targets.as_slice(), device)?.unsqueeze(0)?;
            let log_probs = candle_nn::ops::log_softmax(&logits, 1)?;
            let listwise = (distribution * log_probs)?.sum_all()?.neg()?;
            total = (total + listwise.affine(weights.listwise, 0.0)?)?;
            active_weight += weights.listwise;
        }
        // Binary candidate relevance with grade-weighted positives, in
        // the same weighted-mean form as the unweighted candle BCE.
        if weights.binary_relevance > 0.0 {
            let (targets, sample_weights) =
                binary_relevance_targets(case, &features.candidate_names);
            let flat = logits.squeeze(0)?;
            let target_tensor = Tensor::new(targets.as_slice(), device)?;
            let weight_tensor = Tensor::new(sample_weights.as_slice(), device)?;
            let probs = candle_nn::ops::sigmoid(&flat)?;
            let log_prob = probs.log()?;
            let log_inverse = probs.affine(-1.0, 1.0)?.log()?;
            let positive = (weight_tensor.clone() * target_tensor.clone())?;
            let negative = (weight_tensor * target_tensor.affine(-1.0, 1.0)?)?;
            let loss_sum = ((positive * log_prob)? + (negative * log_inverse)?)?;
            let weight_sum: f32 = sample_weights.iter().sum();
            let binary = loss_sum
                .sum_all()?
                .neg()?
                .affine(1.0 / f64::from(weight_sum.max(1.0)), 0.0)?;
            total = (total + binary.affine(weights.binary_relevance, 0.0)?)?;
            active_weight += weights.binary_relevance;
        }
        // Permutation consistency on mapped-back distributions.
        if weights.consistency > 0.0 {
            if let Some(partner) = consistency_partner {
                let first = candle_nn::ops::softmax(&logits, 1)?;
                let second = candle_nn::ops::softmax(&partner.unsqueeze(0)?, 1)?;
                let consistency = candle_nn::loss::mse(&first, &second)?;
                total = (total + consistency.affine(weights.consistency, 0.0)?)?;
                active_weight += weights.consistency;
            }
        }
    }
    if active_weight <= 0.0 {
        return Err(anyhow!("training objective has no active term"));
    }
    Ok(Tensor::affine(&total, 1.0 / active_weight, 0.0)?)
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
    training_fingerprint: &str,
    dev_fingerprint: &str,
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
        training_partition_fingerprint: training_fingerprint.to_string(),
        dev_partition_fingerprint: dev_fingerprint.to_string(),
        calibration,
        final_weight_sha256,
        seed: config.seed,
        marker_strategy: marker_strategy.into(),
        candidate_representation: candidate_representation.into(),
        batching_strategy: batching_strategy.into(),
        permutation_contract_version: super::order_invariance::PERMUTATION_CONTRACT_VERSION,
        training_objective_version: TRAINING_OBJECTIVE_GRADED_V1.into(),
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

/// Second deterministic presentation of a train case for the
/// consistency term: the second permutation from a seeded generator
/// (identity when the candidate set admits no alternative).
pub fn consistency_partner_case(
    case: &ToolAdvisorCase,
    permutation_seed: u64,
) -> Result<ToolAdvisorCase> {
    use super::order_invariance::{generate_permutations, permute_case};
    let permutations = generate_permutations(
        case.candidates.len(),
        permutation_seed ^ 0x9E37_79B9_7F4A_7C15,
        2,
    );
    let partner = permutations.get(1).unwrap_or(&permutations[0]);
    let mut partnered = permute_case(case, partner)?;
    // Traceable but distinct: partners never re-enter partitioning.
    partnered.case_id = format!("{}::consistency", case.case_id);
    partnered.validate()?;
    Ok(partnered)
}

/// Relevance logits of a consistency partner, realigned to canonical
/// candidate-name order so the MSE compares identical identities.
fn aligned_partner_logits(
    head: &SequenceRankingHead,
    partner_features: &CaseFeatures,
    partner_case: &ToolAdvisorCase,
    canonical_names: &[String],
    device: &Device,
) -> Result<Tensor> {
    let logits = head
        .scores(&partner_features.candidate_vectors, device)?
        .squeeze(0)?
        .to_vec1::<f32>()?;
    if logits.len() != partner_features.candidate_names.len() {
        return Err(anyhow!("partner logit count does not match partner names"));
    }
    let by_name: HashMap<&str, f32> = partner_features
        .candidate_names
        .iter()
        .zip(logits)
        .map(|(name, value)| (name.as_str(), value))
        .collect();
    let mut aligned = Vec::with_capacity(canonical_names.len());
    for name in canonical_names {
        let Some(value) = by_name.get(name.as_str()).copied() else {
            return Err(anyhow!(
                "partner case {} lost candidate {name}",
                partner_case.case_id
            ));
        };
        aligned.push(value);
    }
    Ok(Tensor::new(aligned, device)?)
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
    let train_cases = match config.training_views {
        TrainingViewMode::Canonical => partition
            .train_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect::<Vec<_>>(),
        TrainingViewMode::BalancedPermutation => super::order_invariance::balanced_training_views(
            &cases,
            &partition.train_cases,
            config.permutation_seed,
        )?
        .into_iter()
        .map(|view| view.case)
        .collect::<Vec<_>>(),
    };
    // The training fingerprint covers the exact optimizer input: the
    // canonical partition for canonical runs, the augmented view set
    // (deterministic in seed) for balanced runs.
    let training_fingerprint = super::dataset_fingerprint(&train_cases)?;
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
    // Consistency partners: the same train case under a second
    // deterministic presentation. Partners are derived from the train
    // input (views included), never from dev/test.
    let partner_cases: Option<Vec<ToolAdvisorCase>> = (config.objective.consistency > 0.0)
        .then(|| {
            train_cases
                .iter()
                .map(|case| consistency_partner_case(case, config.permutation_seed))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let cached_partners = matches!(stage, FineTuneStage::HeadOnly)
        .then(|| {
            partner_cases.as_ref().map(|partners| {
                partners
                    .iter()
                    .map(|case| ranker.features(case))
                    .collect::<Result<Vec<_>>>()
            })
        })
        .flatten()
        .transpose()?;
    let mut last_loss = 0.0;
    for _epoch in 0..config.epochs {
        for (index, case) in train_cases.iter().enumerate() {
            let features = match cached_train.as_ref() {
                Some(features) => features[index].clone(),
                None => ranker.features(case)?,
            };
            let partner = match (partner_cases.as_ref(), cached_partners.as_ref()) {
                (Some(partners), Some(cached)) => {
                    let partner_features = cached[index].clone();
                    Some(aligned_partner_logits(
                        &ranker.head,
                        &partner_features,
                        &partners[index],
                        &features.candidate_names,
                        device,
                    )?)
                }
                (Some(partners), None) => {
                    let partner_features = ranker.features(&partners[index])?;
                    Some(aligned_partner_logits(
                        &ranker.head,
                        &partner_features,
                        &partners[index],
                        &features.candidate_names,
                        device,
                    )?)
                }
                (None, _) => None,
            };
            let loss = loss_for_case(
                &ranker.head,
                &features,
                case,
                device,
                &config.objective,
                partner.as_ref(),
            )?;
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
    let (dev_relevance_logits, dev_relevance_labels) =
        dev_relevance_observations(&ranker, &dev_cases).unwrap_or_default();
    let candidate_fit = if dev_relevance_logits.is_empty() {
        CandidateCalibrationFit {
            temperature: 1.0,
            bias: 0.0,
            nll_before: f64::INFINITY,
            nll_after: f64::INFINITY,
        }
    } else {
        fit_candidate_calibration(&dev_relevance_logits, &dev_relevance_labels)
    };
    let calibration = CalibrationParameters {
        method: "dev-dual-calibration-v1".into(),
        abstention_threshold: best_abstention_threshold(
            &dev_cases,
            &ranker,
            cached_dev.as_deref(),
        )?,
        dev_brier: dev_eval.calibration.brier,
        dev_ece: dev_eval.calibration.ece,
        dev_nll: dev_eval.calibration.nll,
        candidate_temperature: candidate_fit.temperature,
        candidate_bias: candidate_fit.bias,
        candidate_nll_before: candidate_fit.nll_before,
        candidate_nll_after: candidate_fit.nll_after,
    };
    let output = PathBuf::from(&config.output_artifact);
    let dev_fingerprint = partition_fingerprint(&cases, &partition.dev_cases)?;
    let _artifact = save_artifact(
        &ranker,
        config,
        &training_fingerprint,
        &dev_fingerprint,
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
        dev_partition_fingerprint: dev_fingerprint,
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
        training_input_mode: match config.training_views {
            TrainingViewMode::Canonical => "canonical".into(),
            TrainingViewMode::BalancedPermutation => "balanced-permutation".into(),
        },
        training_input_fingerprint: training_fingerprint,
        training_objective_version: TRAINING_OBJECTIVE_GRADED_V1.into(),
    })
}

// ---- M003 balanced-training dev selection ----

/// Adapter so M001 mapped-back permutation metrics consume a trained
/// ranker by candidate identity.
struct RankerAsAdvisor<'a> {
    ranker: &'a SequenceRanker,
}

impl ToolAdvisor for RankerAsAdvisor<'_> {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        let case = ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            context: input.context.clone(),
            candidates: input.candidates.clone(),
            relevance: BTreeMap::new(),
            preferred_order: Vec::new(),
            none: true,
            tags: Vec::new(),
            group_id: "dev-selection".into(),
            provenance: "m003-selection".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        };
        let (prediction, _, _) = self.ranker.predict_case(&case)?;
        Ok(prediction)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionBucketMetrics {
    pub target_position: usize,
    pub cases: usize,
    pub mrr: f64,
    pub recall_at_1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermutationSelectionEvidence {
    pub sampled_cases: usize,
    pub permutations_per_case: usize,
    pub mean_top1_consistency: f64,
    pub mean_max_score_drift: f64,
    pub worst_mrr: f64,
    pub best_mrr: f64,
    pub max_regret: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevSelectionMetrics {
    pub mrr: f64,
    pub recall_at_1: f64,
    pub recall_at_3: f64,
    pub recall_at_5: f64,
    pub ndcg_at_5: f64,
    pub no_tool_f1: Option<f64>,
    pub binary_brier: f64,
    pub binary_ece: f64,
    pub binary_nll: f64,
    pub binary_observations: usize,
    pub multi_tool_ndcg: f64,
    pub multi_tool_cases: usize,
    pub unknown_mrr: Option<f64>,
    pub unknown_cases: usize,
    pub hard_negative_mrr: f64,
    pub hard_negative_recall_at_1: f64,
    pub hard_negative_cases: usize,
    pub context_sensitive_mrr: f64,
    pub context_sensitive_recall_at_1: f64,
    pub context_sensitive_cases: usize,
    pub position_buckets: Vec<PositionBucketMetrics>,
    pub permutation: PermutationSelectionEvidence,
}

fn slice_metrics(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
    tag: &str,
) -> Option<(MetricSummary, usize)> {
    let selected: Vec<ToolAdvisorCase> = cases
        .iter()
        .filter(|case| case.tags.iter().any(|t| t == tag))
        .cloned()
        .collect();
    if selected.is_empty() {
        return None;
    }
    let ids: std::collections::BTreeSet<&str> =
        selected.iter().map(|case| case.case_id.as_str()).collect();
    let selected_predictions: Vec<ToolAdvisorPrediction> = predictions
        .iter()
        .filter(|prediction| ids.contains(prediction.case_id.as_str()))
        .cloned()
        .collect();
    Some((
        evaluate(&selected, &selected_predictions).ok()?,
        selected.len(),
    ))
}

/// M003 dev selection metrics for one trained ranker.
///
/// `permutation_sample` bounds the deterministic (case-id-ordered)
/// permutation sample; `permutations_per_case` bounds presentations.
/// Both bounds are recorded in the evidence.
pub fn dev_selection_metrics(
    ranker: &SequenceRanker,
    dev_cases: &[ToolAdvisorCase],
    permutation_sample: usize,
    permutations_per_case: usize,
    permutation_seed: u64,
) -> Result<DevSelectionMetrics> {
    use super::order_invariance::{evaluate_permutation_robustness, summarize_suite, target_index};
    let mut predictions = Vec::with_capacity(dev_cases.len());
    for case in dev_cases {
        let (prediction, _, _) = ranker.predict_case(case)?;
        predictions.push(prediction);
    }
    let summary = evaluate(dev_cases, &predictions)?;
    let (logits, labels) = dev_relevance_observations(ranker, dev_cases)?;
    let calibration = binary_calibration_report(
        &logits,
        &labels,
        ranker.manifest.calibration.candidate_temperature,
        ranker.manifest.calibration.candidate_bias,
    );
    let multi_tool: Vec<ToolAdvisorCase> = dev_cases
        .iter()
        .filter(|case| !case.none && case.relevance.len() > 1)
        .cloned()
        .collect();
    let multi_tool_ndcg = if multi_tool.is_empty() {
        0.0
    } else {
        let ids: std::collections::BTreeSet<&str> = multi_tool
            .iter()
            .map(|case| case.case_id.as_str())
            .collect();
        let subset: Vec<ToolAdvisorPrediction> = predictions
            .iter()
            .filter(|prediction| ids.contains(prediction.case_id.as_str()))
            .cloned()
            .collect();
        evaluate(&multi_tool, &subset)?.ndcg_at_5
    };
    let unknown = slice_metrics(dev_cases, &predictions, "unknown-tool");
    let hard = slice_metrics(dev_cases, &predictions, "hard-negative");
    let sensitive = slice_metrics(dev_cases, &predictions, "context-sensitive");
    // Target-position buckets over non-no-tool dev cases.
    let mut buckets: BTreeMap<usize, Vec<ToolAdvisorCase>> = BTreeMap::new();
    for case in dev_cases {
        if let Some(position) = target_index(case) {
            buckets.entry(position).or_default().push(case.clone());
        }
    }
    let mut position_buckets = Vec::new();
    for (target_position, members) in &buckets {
        let ids: std::collections::BTreeSet<&str> =
            members.iter().map(|case| case.case_id.as_str()).collect();
        let subset: Vec<ToolAdvisorPrediction> = predictions
            .iter()
            .filter(|prediction| ids.contains(prediction.case_id.as_str()))
            .cloned()
            .collect();
        let metrics = evaluate(members, &subset)?;
        position_buckets.push(PositionBucketMetrics {
            target_position: *target_position,
            cases: members.len(),
            mrr: metrics.mrr,
            recall_at_1: metrics.recall_at_1,
        });
    }
    // Deterministic permutation sample (case-id order).
    let mut ordered = dev_cases.to_vec();
    ordered.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    let advisor = RankerAsAdvisor { ranker };
    let mut per_case = Vec::new();
    for case in ordered.iter().take(permutation_sample.max(1)) {
        per_case.push(evaluate_permutation_robustness(
            case,
            &advisor,
            permutation_seed,
            permutations_per_case.max(1),
        )?);
    }
    let suite = summarize_suite(&per_case);
    Ok(DevSelectionMetrics {
        mrr: summary.mrr,
        recall_at_1: summary.recall_at_1,
        recall_at_3: summary.recall_at_3,
        recall_at_5: summary.recall_at_5,
        ndcg_at_5: summary.ndcg_at_5,
        no_tool_f1: summary.no_tool_f1,
        binary_brier: calibration.brier,
        binary_ece: calibration.ece,
        binary_nll: calibration.nll,
        binary_observations: calibration.observations,
        multi_tool_ndcg,
        multi_tool_cases: multi_tool.len(),
        unknown_mrr: unknown.as_ref().map(|(metrics, _)| metrics.mrr),
        unknown_cases: unknown.as_ref().map(|(_, count)| *count).unwrap_or(0),
        hard_negative_mrr: hard.as_ref().map(|(metrics, _)| metrics.mrr).unwrap_or(0.0),
        hard_negative_recall_at_1: hard
            .as_ref()
            .map(|(metrics, _)| metrics.recall_at_1)
            .unwrap_or(0.0),
        hard_negative_cases: hard.as_ref().map(|(_, count)| *count).unwrap_or(0),
        context_sensitive_mrr: sensitive
            .as_ref()
            .map(|(metrics, _)| metrics.mrr)
            .unwrap_or(0.0),
        context_sensitive_recall_at_1: sensitive
            .as_ref()
            .map(|(metrics, _)| metrics.recall_at_1)
            .unwrap_or(0.0),
        context_sensitive_cases: sensitive.as_ref().map(|(_, count)| *count).unwrap_or(0),
        position_buckets,
        permutation: PermutationSelectionEvidence {
            sampled_cases: suite.cases,
            permutations_per_case,
            mean_top1_consistency: suite.mean_top1_consistency,
            mean_max_score_drift: suite.mean_max_score_drift,
            worst_mrr: suite.worst_mrr,
            best_mrr: suite.best_mrr,
            max_regret: suite.max_regret,
        },
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionGate {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionVerdict {
    pub eligible: bool,
    pub gates: Vec<SelectionGate>,
}

/// Frozen linear (or declared fallback) reference for §8 gates.
#[derive(Debug, Clone)]
pub struct SelectionBaseline {
    pub label: String,
    pub mrr: f64,
    pub recall_at_1: f64,
    pub no_tool_f1: Option<f64>,
    pub multi_tool_ndcg: f64,
    pub hard_negative_mrr: f64,
    pub hard_negative_recall_at_1: f64,
    pub context_sensitive_mrr: f64,
    pub context_sensitive_recall_at_1: f64,
    pub unknown_mrr: Option<f64>,
}

/// M003 §8 dev selection gates. All thresholds are plan-literal.
pub fn check_selection_gates(
    metrics: &DevSelectionMetrics,
    baseline: &SelectionBaseline,
) -> SelectionVerdict {
    let mut gates = Vec::new();
    let consistency = metrics.permutation.mean_top1_consistency;
    gates.push(SelectionGate {
        id: "permutation-consistency".into(),
        passed: consistency >= 0.95,
        detail: format!("top-1 identity consistency {consistency:.3} >= 0.95"),
    });
    let mut position_gate = true;
    let mut position_detail = String::from("no bucket with >= 10 cases");
    for bucket in &metrics.position_buckets {
        if bucket.cases >= 10 {
            let gap = metrics.recall_at_1 - bucket.recall_at_1;
            position_detail = format!(
                "position {}: {} cases, R1 gap {gap:.3}",
                bucket.target_position, bucket.cases
            );
            if gap > 0.05 {
                position_gate = false;
            }
        }
    }
    gates.push(SelectionGate {
        id: "position-stratified-recall".into(),
        passed: position_gate,
        detail: position_detail,
    });
    gates.push(SelectionGate {
        id: "aggregate-mrr".into(),
        passed: metrics.mrr >= baseline.mrr - 0.01,
        detail: format!(
            "dev MRR {:.3} vs {} {:.3} - 0.01",
            metrics.mrr, baseline.label, baseline.mrr
        ),
    });
    let hard_gain = (metrics.hard_negative_mrr - baseline.hard_negative_mrr >= 0.02
        || metrics.hard_negative_recall_at_1 - baseline.hard_negative_recall_at_1 >= 0.02)
        && metrics.hard_negative_cases > 0;
    let sensitive_gain = (metrics.context_sensitive_mrr - baseline.context_sensitive_mrr >= 0.02
        || metrics.context_sensitive_recall_at_1 - baseline.context_sensitive_recall_at_1 >= 0.02)
        && metrics.context_sensitive_cases > 0;
    let unknown_gain = match (metrics.unknown_mrr, baseline.unknown_mrr) {
        (Some(mrr), Some(base)) => mrr - base >= 0.02 && metrics.unknown_cases > 0,
        _ => false,
    };
    gates.push(SelectionGate {
        id: "hard-slice-gain".into(),
        passed: hard_gain || sensitive_gain || unknown_gain,
        detail: format!(
            "hard-negative {}/{:.3}/{:.3}, context-sensitive {}/{:.3}/{:.3}, unknown {:?}",
            metrics.hard_negative_cases,
            metrics.hard_negative_mrr,
            metrics.hard_negative_recall_at_1,
            metrics.context_sensitive_cases,
            metrics.context_sensitive_mrr,
            metrics.context_sensitive_recall_at_1,
            metrics.unknown_mrr,
        ),
    });
    let no_tool_gate = match (metrics.no_tool_f1, baseline.no_tool_f1) {
        (Some(mine), Some(base)) => mine >= base - 0.02,
        (None, None) => true,
        (Some(_), None) => true,
        (None, Some(_)) => false,
    };
    gates.push(SelectionGate {
        id: "no-tool-f1".into(),
        passed: no_tool_gate,
        detail: format!(
            "dev {:?} vs {} {:?}",
            metrics.no_tool_f1, baseline.label, baseline.no_tool_f1
        ),
    });
    gates.push(SelectionGate {
        id: "multi-tool-ndcg".into(),
        passed: metrics.multi_tool_ndcg >= baseline.multi_tool_ndcg - 0.02,
        detail: format!(
            "multi-tool nDCG {:.3} ({} cases) vs {} {:.3} - 0.02",
            metrics.multi_tool_ndcg,
            metrics.multi_tool_cases,
            baseline.label,
            baseline.multi_tool_ndcg
        ),
    });
    let calibration_gate = metrics.binary_brier.is_finite()
        && metrics.binary_nll.is_finite()
        && metrics.binary_observations > 0;
    gates.push(SelectionGate {
        id: "relevance-calibration".into(),
        passed: calibration_gate,
        detail: format!(
            "Brier {:.4} NLL {:.4} over {} observations",
            metrics.binary_brier, metrics.binary_nll, metrics.binary_observations
        ),
    });
    gates.push(SelectionGate {
        id: "authority-runtime-invariant".into(),
        passed: true,
        detail:
            "ranker holds no authority path by construction; retrieval/promotion unchanged in M003"
                .into(),
    });
    SelectionVerdict {
        eligible: gates.iter().all(|gate| gate.passed),
        gates,
    }
}

/// One predeclared M003 sweep arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionArmConfig {
    pub name: String,
    pub architecture: RankingArchitecture,
    pub pooling: PoolingStrategy,
    pub stage: String,
    pub epochs: u32,
    pub learning_rate: f64,
    pub max_candidates: usize,
    pub max_tokens: usize,
    pub objective: ObjectiveWeights,
    pub training_views: TrainingViewMode,
    pub permutation_seed: u64,
}

/// Predeclared M003 sweep: finite arms, fixed dev protocol, declared
/// baseline source. The sweep fingerprint binds the exact attempt set;
/// every attempt (including failures) is recorded in the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionSweepConfig {
    pub schema_version: u16,
    pub protocol: String,
    pub manifest_path: String,
    pub dataset: String,
    pub output_dir: String,
    pub seed: u64,
    pub arms: Vec<SelectionArmConfig>,
    pub dev_permutation_sample: usize,
    pub dev_permutations_per_case: usize,
    /// Frozen hashed-linear artifact for the §8 baseline when present;
    /// otherwise the portable BM25 baseline is used and recorded.
    pub linear_baseline_artifact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmSelectionResult {
    pub arm: String,
    pub architecture: String,
    pub artifact: String,
    pub train_report: SequenceRankingExperimentReport,
    pub dev: DevSelectionMetrics,
    pub verdict: SelectionVerdict,
    pub baseline_label: String,
    pub baseline_mrr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3SelectionDiagnostic {
    pub cases: usize,
    pub sequence_mrr: f64,
    pub sequence_recall_at_1: f64,
    pub baseline_mrr: f64,
    pub baseline_recall_at_1: f64,
    pub non_gating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M003SelectionReport {
    pub schema_version: u16,
    pub protocol: String,
    pub sweep_fingerprint: String,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub arms: Vec<ArmSelectionResult>,
    pub failures: Vec<String>,
    pub selected_arm: Option<String>,
    pub selected_artifact: Option<String>,
    pub v3_diagnostic: Option<V3SelectionDiagnostic>,
}

fn sweep_fingerprint(config: &SelectionSweepConfig) -> Result<String> {
    let canonical = serde_json::to_string(config)?;
    Ok(hex::encode(Sha256::digest(canonical.as_bytes())))
}

fn arm_training_config(
    sweep: &SelectionSweepConfig,
    arm: &SelectionArmConfig,
) -> SequenceRankingConfig {
    SequenceRankingConfig {
        schema_version: RANKING_SCHEMA_VERSION,
        manifest_path: sweep.manifest_path.clone(),
        dataset: sweep.dataset.clone(),
        output_artifact: format!("{}/{}.json", sweep.output_dir, arm.name),
        architecture: arm.architecture,
        pooling: arm.pooling,
        stage: arm.stage.clone(),
        epochs: arm.epochs,
        learning_rate: arm.learning_rate,
        max_candidates: arm.max_candidates,
        max_tokens: arm.max_tokens,
        seed: sweep.seed,
        objective: arm.objective,
        training_views: arm.training_views,
        permutation_seed: arm.permutation_seed,
    }
}

/// Baseline for §8 gates: the frozen hashed-linear artifact when it
/// loads, otherwise portable BM25. The label records which one gated.
fn selection_baseline(
    dev_cases: &[ToolAdvisorCase],
    linear_artifact: Option<&str>,
) -> Result<(SelectionBaseline, String)> {
    let multi_ndcg_of =
        |cases: &[ToolAdvisorCase], predictions: &[ToolAdvisorPrediction]| -> Result<f64> {
            let multi: Vec<ToolAdvisorCase> = cases
                .iter()
                .filter(|case| !case.none && case.relevance.len() > 1)
                .cloned()
                .collect();
            if multi.is_empty() {
                return Ok(0.0);
            }
            let ids: std::collections::BTreeSet<&str> =
                multi.iter().map(|case| case.case_id.as_str()).collect();
            let subset: Vec<ToolAdvisorPrediction> = predictions
                .iter()
                .filter(|prediction| ids.contains(prediction.case_id.as_str()))
                .cloned()
                .collect();
            Ok(evaluate(&multi, &subset)?.ndcg_at_5)
        };
    let slice_of = |cases: &[ToolAdvisorCase], predictions: &[ToolAdvisorPrediction], tag: &str| {
        slice_metrics(cases, predictions, tag)
            .map(|(metrics, _)| (metrics.mrr, metrics.recall_at_1))
            .unwrap_or((0.0, 0.0))
    };
    if let Some(path) = linear_artifact {
        if let Ok(artifact) =
            super::load_artifact(Path::new(path)).and_then(super::LinearAdvisor::new)
        {
            let predictions: Vec<ToolAdvisorPrediction> = dev_cases
                .iter()
                .map(|case| {
                    artifact.score(&ToolAdvisorInput {
                        case_id: case.case_id.clone(),
                        context: case.context.clone(),
                        candidates: case.candidates.clone(),
                        surface_fingerprint: String::new(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let summary = evaluate(dev_cases, &predictions)?;
            let (hard_mrr, hard_r1) = slice_of(dev_cases, &predictions, "hard-negative");
            let (sensitive_mrr, sensitive_r1) =
                slice_of(dev_cases, &predictions, "context-sensitive");
            let unknown_mrr = slice_metrics(dev_cases, &predictions, "unknown-tool")
                .map(|(metrics, _)| metrics.mrr);
            return Ok((
                SelectionBaseline {
                    label: "frozen-linear".into(),
                    mrr: summary.mrr,
                    recall_at_1: summary.recall_at_1,
                    no_tool_f1: summary.no_tool_f1,
                    multi_tool_ndcg: multi_ndcg_of(dev_cases, &predictions)?,
                    hard_negative_mrr: hard_mrr,
                    hard_negative_recall_at_1: hard_r1,
                    context_sensitive_mrr: sensitive_mrr,
                    context_sensitive_recall_at_1: sensitive_r1,
                    unknown_mrr,
                },
                "frozen-linear".into(),
            ));
        }
    }
    let predictions: Vec<ToolAdvisorPrediction> = dev_cases
        .iter()
        .map(|case| baseline_prediction(case, crate::tool::catalog::SearchMode::BM25))
        .collect();
    let summary = evaluate(dev_cases, &predictions)?;
    let (hard_mrr, hard_r1) = slice_of(dev_cases, &predictions, "hard-negative");
    let (sensitive_mrr, sensitive_r1) = slice_of(dev_cases, &predictions, "context-sensitive");
    let unknown_mrr =
        slice_metrics(dev_cases, &predictions, "unknown-tool").map(|(metrics, _)| metrics.mrr);
    Ok((
        SelectionBaseline {
            label: "bm25".into(),
            mrr: summary.mrr,
            recall_at_1: summary.recall_at_1,
            no_tool_f1: summary.no_tool_f1,
            multi_tool_ndcg: multi_ndcg_of(dev_cases, &predictions)?,
            hard_negative_mrr: hard_mrr,
            hard_negative_recall_at_1: hard_r1,
            context_sensitive_mrr: sensitive_mrr,
            context_sensitive_recall_at_1: sensitive_r1,
            unknown_mrr,
        },
        "bm25".into(),
    ))
}

/// Run the predeclared M003 sweep: train every arm, evaluate dev
/// selection metrics, apply §8 gates, select at most one artifact,
/// then run the v3 diagnostic exactly once after selection without
/// feeding anything back.
pub fn run_selection_sweep(
    sweep: &SelectionSweepConfig,
    device: &Device,
) -> Result<M003SelectionReport> {
    if sweep.schema_version != 1 {
        return Err(anyhow!("unsupported selection sweep schema"));
    }
    if sweep.arms.is_empty() {
        return Err(anyhow!("selection sweep declares no arms"));
    }
    let fingerprint = sweep_fingerprint(sweep)?;
    let cases = load_cases(Some(Path::new(&sweep.dataset)))?;
    let dataset_fp = dataset_fingerprint(&cases)?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let (baseline, baseline_label) =
        selection_baseline(&dev_cases, sweep.linear_baseline_artifact.as_deref())?;
    std::fs::create_dir_all(&sweep.output_dir)?;
    let mut arms = Vec::new();
    let mut failures = Vec::new();
    for arm in &sweep.arms {
        let arm_config = arm_training_config(sweep, arm);
        match train(&arm_config, device).and_then(|train_report| {
            let ranker = load_artifact(Path::new(&arm_config.output_artifact), device)?;
            let dev = dev_selection_metrics(
                &ranker,
                &dev_cases,
                sweep.dev_permutation_sample,
                sweep.dev_permutations_per_case,
                arm.permutation_seed,
            )?;
            Ok((train_report, dev))
        }) {
            Ok((train_report, dev)) => {
                let verdict = check_selection_gates(&dev, &baseline);
                arms.push(ArmSelectionResult {
                    arm: arm.name.clone(),
                    architecture: arm.architecture.as_str().into(),
                    artifact: arm_config.output_artifact.clone(),
                    train_report,
                    dev,
                    verdict,
                    baseline_label: baseline_label.clone(),
                    baseline_mrr: baseline.mrr,
                });
            }
            Err(error) => failures.push(format!("{}: {error:#}", arm.name)),
        }
    }
    if arms.is_empty() {
        return Err(anyhow!(
            "selection sweep trained no arm; failures: {}",
            failures.join("; ")
        ));
    }
    // Select the eligible arm with the best dev MRR, breaking ties by
    // permutation consistency and then arm name (deterministic).
    let mut eligible: Vec<&ArmSelectionResult> = arms
        .iter()
        .filter(|result| result.verdict.eligible)
        .collect();
    eligible.sort_by(|left, right| {
        right
            .dev
            .mrr
            .partial_cmp(&left.dev.mrr)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                right
                    .dev
                    .permutation
                    .mean_top1_consistency
                    .partial_cmp(&left.dev.permutation.mean_top1_consistency)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| left.arm.cmp(&right.arm))
    });
    let (selected_arm, selected_artifact) = eligible
        .first()
        .map(|result| (Some(result.arm.clone()), Some(result.artifact.clone())))
        .unwrap_or((None, None));
    // Post-selection v3 diagnostic only: never feeds back into
    // training, thresholds, architecture, or M004 choices.
    let v3_diagnostic = match &selected_artifact {
        Some(path) => {
            let v3_path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/tool-advisor/qualification-v3-holdout.jsonl");
            let v3_cases =
                super::parse_jsonl(&std::fs::read_to_string(&v3_path).context("read v3 holdout")?)?;
            let ranker = load_artifact(Path::new(path), device)?;
            let mut predictions = Vec::with_capacity(v3_cases.len());
            for case in &v3_cases {
                let (prediction, _, _) = ranker.predict_case(case)?;
                predictions.push(prediction);
            }
            let sequence = evaluate(&v3_cases, &predictions)?;
            let baseline_predictions: Vec<ToolAdvisorPrediction> = v3_cases
                .iter()
                .map(|case| baseline_prediction(case, crate::tool::catalog::SearchMode::BM25))
                .collect();
            let baseline_metrics = evaluate(&v3_cases, &baseline_predictions)?;
            Some(V3SelectionDiagnostic {
                cases: v3_cases.len(),
                sequence_mrr: sequence.mrr,
                sequence_recall_at_1: sequence.recall_at_1,
                baseline_mrr: baseline_metrics.mrr,
                baseline_recall_at_1: baseline_metrics.recall_at_1,
                non_gating: true,
            })
        }
        None => None,
    };
    Ok(M003SelectionReport {
        schema_version: 1,
        protocol: sweep.protocol.clone(),
        sweep_fingerprint: fingerprint,
        dataset_fingerprint: dataset_fp,
        train_partition_fingerprint: partition_fingerprint(&cases, &partition.train_cases)?,
        dev_partition_fingerprint: partition_fingerprint(&cases, &partition.dev_cases)?,
        arms,
        failures,
        selected_arm,
        selected_artifact,
        v3_diagnostic,
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
        training_objective_version: TRAINING_OBJECTIVE_GRADED_V1.into(),
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
    use super::super::{order_invariance, ToolAdvisorCandidate};
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

    fn m003_sample_case() -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: "m003-sample".into(),
            context: "read a file".into(),
            candidates: vec![
                ToolAdvisorCandidate {
                    name: "read".into(),
                    description: "Read files".into(),
                    category: String::new(),
                    disclosure: String::new(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "write".into(),
                    description: "Write files".into(),
                    category: String::new(),
                    disclosure: String::new(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "shell".into(),
                    description: "Run shell".into(),
                    category: String::new(),
                    disclosure: String::new(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "grep".into(),
                    description: "Search files".into(),
                    category: String::new(),
                    disclosure: String::new(),
                    synthetic_identity: false,
                },
            ],
            relevance: BTreeMap::from([("read".to_string(), 3), ("write".to_string(), 3)]),
            preferred_order: vec!["read".into(), "write".into()],
            none: false,
            tags: Vec::new(),
            group_id: "m003-group".into(),
            provenance: "m003".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    /// M003 §4: two equally relevant candidates share the graded mass
    /// instead of collapsing to one target index.
    #[test]
    fn graded_targets_preserve_multiple_relevant() {
        let case = m003_sample_case();
        let names: Vec<String> = case.candidates.iter().map(|c| c.name.clone()).collect();
        let distribution = graded_target_distribution(&case, &names);
        // Gains 7, 7, 0, 0 over total 14.
        assert_eq!(distribution.len(), 4);
        assert!((distribution[0] - 0.5).abs() < 1e-6);
        assert!((distribution[1] - 0.5).abs() < 1e-6);
        assert_eq!(distribution[2], 0.0);
        assert_eq!(distribution[3], 0.0);
        assert!((distribution.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    /// M003 §4: binary targets mark every relevant candidate positive
    /// with grade weights, so both can receive high probability.
    #[test]
    fn binary_targets_mark_positives_with_grade_weights() {
        let case = m003_sample_case();
        let names: Vec<String> = case.candidates.iter().map(|c| c.name.clone()).collect();
        let (targets, weights) = binary_relevance_targets(&case, &names);
        assert_eq!(targets, vec![1.0, 1.0, 0.0, 0.0]);
        assert_eq!(weights, vec![3.0, 3.0, 1.0, 1.0]);
    }

    /// M003 §5: the temperature+bias grid can never be worse than the
    /// identity because (1.0, 0.0) is always a grid point.
    #[test]
    fn candidate_calibration_fit_never_worse_than_identity() {
        let logits = vec![2.0, 1.0, -1.0, -2.0, 0.5, -0.5];
        let labels = vec![1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let fit = fit_candidate_calibration(&logits, &labels);
        assert!(fit.nll_after <= fit.nll_before);
        let report = binary_calibration_report(&logits, &labels, fit.temperature, fit.bias);
        assert_eq!(report.observations, 6);
        assert!(report.brier.is_finite() && report.nll.is_finite());
        assert!((0.0..=1.0).contains(&report.ece));
        // Identity calibration of a perfect separator is already good.
        let perfect = fit_candidate_calibration(&[3.0, -3.0], &[1.0, 0.0]);
        assert!(perfect.nll_after <= perfect.nll_before);
    }

    /// M003 §9: new training fields default safely; sweep configs pin
    /// them explicitly.
    #[test]
    fn training_view_mode_defaults_to_canonical() {
        let minimal = serde_json::json!({
            "schema_version": 1,
            "manifest_path": "x",
            "output_artifact": "y",
        });
        let config: SequenceRankingConfig = serde_json::from_value(minimal).unwrap();
        assert_eq!(config.training_views, TrainingViewMode::Canonical);
        assert_eq!(config.objective.listwise, 1.0);
        assert_eq!(config.objective.binary_relevance, 1.0);
        assert_eq!(config.objective.abstention, 1.0);
        assert_eq!(config.objective.consistency, 0.0);
        assert_eq!(config.permutation_seed, order_invariance::TRAIN_VIEW_SEED);
        let explicit = serde_json::json!({
            "schema_version": 1,
            "manifest_path": "x",
            "output_artifact": "y",
            "training_views": "balanced-permutation",
        });
        let config: SequenceRankingConfig = serde_json::from_value(explicit).unwrap();
        assert_eq!(config.training_views, TrainingViewMode::BalancedPermutation);
    }

    /// M003: consistency partners are deterministic, traceable, and
    /// label-preserving.
    #[test]
    fn consistency_partner_is_deterministic_and_traceable() {
        let case = m003_sample_case();
        let first = consistency_partner_case(&case, 99).unwrap();
        let second = consistency_partner_case(&case, 99).unwrap();
        assert_eq!(first.candidates, second.candidates);
        assert_eq!(first.case_id, "m003-sample::consistency");
        assert_eq!(first.relevance, case.relevance);
        assert_eq!(first.none, case.none);
        assert_eq!(first.semantic_group, case.semantic_group);
        let names: std::collections::BTreeSet<&str> =
            first.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), 4, "partner must preserve every identity");
    }

    fn passing_selection_metrics() -> DevSelectionMetrics {
        DevSelectionMetrics {
            mrr: 0.70,
            recall_at_1: 0.60,
            recall_at_3: 0.75,
            recall_at_5: 0.80,
            ndcg_at_5: 0.72,
            no_tool_f1: Some(0.60),
            binary_brier: 0.15,
            binary_ece: 0.05,
            binary_nll: 0.45,
            binary_observations: 200,
            multi_tool_ndcg: 0.70,
            multi_tool_cases: 18,
            unknown_mrr: Some(0.55),
            unknown_cases: 8,
            hard_negative_mrr: 0.60,
            hard_negative_recall_at_1: 0.50,
            hard_negative_cases: 15,
            context_sensitive_mrr: 0.65,
            context_sensitive_recall_at_1: 0.55,
            context_sensitive_cases: 19,
            position_buckets: vec![PositionBucketMetrics {
                target_position: 0,
                cases: 50,
                mrr: 0.70,
                recall_at_1: 0.60,
            }],
            permutation: PermutationSelectionEvidence {
                sampled_cases: 16,
                permutations_per_case: 8,
                mean_top1_consistency: 0.97,
                mean_max_score_drift: 0.05,
                worst_mrr: 0.65,
                best_mrr: 0.72,
                max_regret: 0.07,
            },
        }
    }

    fn selection_test_baseline() -> SelectionBaseline {
        SelectionBaseline {
            label: "test".into(),
            mrr: 0.65,
            recall_at_1: 0.55,
            no_tool_f1: Some(0.58),
            multi_tool_ndcg: 0.68,
            hard_negative_mrr: 0.55,
            hard_negative_recall_at_1: 0.45,
            context_sensitive_mrr: 0.60,
            context_sensitive_recall_at_1: 0.50,
            unknown_mrr: Some(0.53),
        }
    }

    /// M003 §8: gates implement the plan thresholds literally.
    #[test]
    fn selection_gates_require_all_plan_thresholds() {
        let baseline = selection_test_baseline();
        let verdict = check_selection_gates(&passing_selection_metrics(), &baseline);
        assert!(
            verdict.eligible,
            "passing metrics must be eligible: {:?}",
            verdict.gates
        );
        assert_eq!(verdict.gates.len(), 8);
        // Consistency below 0.95 fails.
        let mut metrics = passing_selection_metrics();
        metrics.permutation.mean_top1_consistency = 0.90;
        assert!(!check_selection_gates(&metrics, &baseline).eligible);
        // MRR more than 0.01 below baseline fails.
        let mut metrics = passing_selection_metrics();
        metrics.mrr = 0.60;
        assert!(!check_selection_gates(&metrics, &baseline).eligible);
        // A >= 10-case bucket > 0.05 R1 below aggregate fails.
        let mut metrics = passing_selection_metrics();
        metrics.position_buckets.push(PositionBucketMetrics {
            target_position: 1,
            cases: 12,
            mrr: 0.40,
            recall_at_1: 0.30,
        });
        assert!(!check_selection_gates(&metrics, &baseline).eligible);
        // Small buckets are exempt.
        let mut metrics = passing_selection_metrics();
        metrics.position_buckets.push(PositionBucketMetrics {
            target_position: 1,
            cases: 2,
            mrr: 0.0,
            recall_at_1: 0.0,
        });
        assert!(check_selection_gates(&metrics, &baseline).eligible);
        // No hard-slice gain fails (all slices at/below baseline).
        let mut metrics = passing_selection_metrics();
        metrics.hard_negative_mrr = 0.55;
        metrics.hard_negative_recall_at_1 = 0.45;
        metrics.context_sensitive_mrr = 0.60;
        metrics.context_sensitive_recall_at_1 = 0.50;
        metrics.unknown_mrr = Some(0.53);
        assert!(!check_selection_gates(&metrics, &baseline).eligible);
    }

    /// Predeclared M003 sweep (protocol
    /// `m003-preregistered-dev-selection-v1`).
    ///
    /// Four head-only arms (v1 control, shared packed, span packed,
    /// batched pairwise) train on M001 balanced views with the
    /// corrected graded objective; dev selection applies the §8 gates.
    /// Run explicitly for closure evidence:
    /// `cargo test --locked --features tool-advisor-encoder-training
    /// -p codegg --lib
    /// tool_advisor::sequence_ranking::tests::m003_selection_sweep --
    /// --ignored --nocapture`
    fn m003_sweep_config(output_dir: &str) -> SelectionSweepConfig {
        let arm = |name: &str, architecture: RankingArchitecture| SelectionArmConfig {
            name: name.into(),
            architecture,
            pooling: PoolingStrategy::Mean,
            stage: "head-only".into(),
            epochs: 3,
            learning_rate: 1e-3,
            max_candidates: 16,
            max_tokens: 256,
            objective: ObjectiveWeights {
                listwise: 1.0,
                binary_relevance: 1.0,
                abstention: 1.0,
                consistency: 0.0,
            },
            training_views: TrainingViewMode::BalancedPermutation,
            permutation_seed: order_invariance::TRAIN_VIEW_SEED,
        };
        SelectionSweepConfig {
            schema_version: 1,
            protocol: "m003-preregistered-dev-selection-v1".into(),
            manifest_path: "target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json"
                .into(),
            dataset: "assets/tool-advisor/corpus.jsonl".into(),
            output_dir: output_dir.into(),
            seed: 7,
            arms: vec![
                arm("v1-control", RankingArchitecture::Packed),
                arm("shared-packed", RankingArchitecture::SharedPacked),
                arm("span-packed", RankingArchitecture::SpanPacked),
                arm("batched-pairwise", RankingArchitecture::BatchedPairwise),
            ],
            dev_permutation_sample: 16,
            dev_permutations_per_case: 8,
            linear_baseline_artifact: Some("target/tool-advisor-smoke/model.json".into()),
        }
    }

    #[test]
    #[ignore]
    fn m003_selection_sweep() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        if !root
            .join("target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json")
            .exists()
        {
            eprintln!("SKIP: reference encoder assets absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let out_dir = root.join("target/tool-advisor/order-invariance/m003-arms");
        let sweep = m003_sweep_config(&out_dir.display().to_string());
        let mut report = run_selection_sweep(&sweep, &device).expect("selection sweep completes");
        eprintln!("sweep fingerprint: {}", report.sweep_fingerprint);
        for arm in &report.arms {
            let verdict = if arm.verdict.eligible {
                "ELIGIBLE"
            } else {
                "ineligible"
            };
            eprintln!(
                "{} [{}] dev MRR {:.4} R1 {:.4} no-tool {:?} multi-nDCG {:.4} consistency {:.3} -> {verdict}",
                arm.arm,
                arm.architecture,
                arm.dev.mrr,
                arm.dev.recall_at_1,
                arm.dev.no_tool_f1,
                arm.dev.multi_tool_ndcg,
                arm.dev.permutation.mean_top1_consistency,
            );
            for gate in &arm.verdict.gates {
                eprintln!(
                    "    gate {:28} {} {}",
                    gate.id,
                    if gate.passed { "pass" } else { "FAIL" },
                    gate.detail
                );
            }
        }
        for failure in &report.failures {
            eprintln!("arm failure: {failure}");
        }
        // Predeclared conditional branch: a packed arm that passes every
        // quality gate but fails ONLY permutation consistency gets one
        // consistency-0.1 follow-up. Anything else closes as-is.
        let mut followups = Vec::new();
        for arm in &report.arms {
            let failed: Vec<&str> = arm
                .verdict
                .gates
                .iter()
                .filter(|gate| !gate.passed)
                .map(|gate| gate.id.as_str())
                .collect();
            if failed == ["permutation-consistency"]
                && (arm.architecture == RANKING_ARCHITECTURE_SHARED_PACKED
                    || arm.architecture == RANKING_ARCHITECTURE_SPAN_PACKED)
            {
                followups.push(arm.arm.clone());
            }
        }
        if !followups.is_empty() {
            eprintln!("conditional consistency follow-ups: {followups:?}");
            let mut second = m003_sweep_config(&out_dir.display().to_string());
            second.protocol = "m003-preregistered-dev-selection-v1-followup".into();
            second.arms = sweep
                .arms
                .iter()
                .filter(|arm| followups.contains(&arm.name))
                .map(|arm| {
                    let mut arm = arm.clone();
                    arm.name = format!("{}-consistency-01", arm.name);
                    arm.objective.consistency = 0.1;
                    arm
                })
                .collect();
            // Follow-up arms train from scratch with the consistency
            // term active from the first step (no warm start).
            let second_report =
                run_selection_sweep(&second, &device).expect("follow-up sweep completes");
            std::fs::write(
                out_dir.join("m003-selection-followup.json"),
                serde_json::to_vec_pretty(&second_report).expect("serialize"),
            )
            .expect("write follow-up report");
            report.arms.extend(second_report.arms);
            report.failures.extend(second_report.failures);
            // Re-select across the merged arm set by the same rule.
            let mut eligible: Vec<&ArmSelectionResult> = report
                .arms
                .iter()
                .filter(|result| result.verdict.eligible)
                .collect();
            eligible.sort_by(|left, right| {
                right
                    .dev
                    .mrr
                    .partial_cmp(&left.dev.mrr)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        right
                            .dev
                            .permutation
                            .mean_top1_consistency
                            .partial_cmp(&left.dev.permutation.mean_top1_consistency)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left.arm.cmp(&right.arm))
            });
            // The v3 diagnostic stays bound to the primary sweep
            // selection (post-selection, non-gating); follow-up
            // comparison is dev-only and recorded per arm above.
            let (selected_arm, selected_artifact) = eligible
                .first()
                .map(|result| (Some(result.arm.clone()), Some(result.artifact.clone())))
                .unwrap_or((None, None));
            report.selected_arm = selected_arm;
            report.selected_artifact = selected_artifact;
        }
        eprintln!(
            "selected: {:?} @ {:?}; v3 diagnostic: {:?}",
            report.selected_arm, report.selected_artifact, report.v3_diagnostic
        );
        std::fs::write(
            out_dir.join("m003-selection.json"),
            serde_json::to_vec_pretty(&report).expect("serialize"),
        )
        .expect("write selection report");
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
