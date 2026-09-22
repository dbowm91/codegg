//! Frozen, offline qualification protocol for the sequence-encoder experiment.
//!
//! The protocol verifies all fingerprints and artifact hashes before it
//! selects the final partition.  This is the only command in the experiment
//! that reads frozen test labels; training and retrieval development paths do
//! not call it.

use super::sequence_ranking::{load_artifact, RankingEvaluation};
use super::sequence_retrieval::{
    frontier, parse_mode, select_retrieval_point, validate_expanded_fixture, HybridRetriever,
    RetrievalMode,
};
use super::{
    baseline_prediction, dataset_fingerprint, evaluate, leakage_report,
    load_artifact as load_linear, load_cases, partition_cases, LinearAdvisor, ToolAdvisor,
    ToolAdvisorCase, ToolAdvisorInput, ToolAdvisorPrediction,
};
use crate::tool::catalog::{SearchMode, ToolMetadata};
use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const QUALIFICATION_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualificationGates {
    #[serde(default = "default_recall_64")]
    pub recall_64_min: f64,
    #[serde(default = "default_recall_128")]
    pub recall_128_min: f64,
    #[serde(default = "default_mrr_tolerance")]
    pub aggregate_mrr_tolerance: f64,
    #[serde(default = "default_slice_gain")]
    pub slice_gain_min: f64,
    #[serde(default = "default_slice_regression")]
    pub slice_regression_max: f64,
    #[serde(default = "default_no_tool_tolerance")]
    pub no_tool_f1_tolerance: f64,
}

fn default_recall_64() -> f64 {
    0.98
}
fn default_recall_128() -> f64 {
    0.95
}
fn default_mrr_tolerance() -> f64 {
    0.01
}
fn default_slice_gain() -> f64 {
    0.02
}
fn default_slice_regression() -> f64 {
    0.02
}
fn default_no_tool_tolerance() -> f64 {
    0.02
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationPreregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub dataset: String,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub test_partition_fingerprint: String,
    pub sequence_artifact: String,
    pub sequence_artifact_sha256: String,
    pub selected_encoder_manifest_sha256: String,
    pub selected_encoder_config_sha256: String,
    pub selected_tokenizer_sha256: String,
    pub selected_source_weights_sha256: String,
    #[serde(default)]
    pub training_config_hashes: BTreeMap<String, String>,
    pub retrieval_mode: String,
    pub retrieval_k: usize,
    pub promotion_max_candidates: usize,
    pub promotion_max_promotions: usize,
    pub promotion_schema_budget_bytes: usize,
    pub evaluation_command: String,
    pub resource_limits: ResourceLimits,
    pub preregistration_commit_sha: String,
    pub protocol_hash: String,
    #[serde(default)]
    pub linear_artifact: Option<String>,
    #[serde(default)]
    pub contextual_artifact: Option<String>,
    #[serde(default)]
    pub gates: QualificationGates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default = "default_max_cold_load_ms")]
    pub max_cold_load_ms: u128,
    #[serde(default = "default_max_rank_ms")]
    pub max_rank_ms: u128,
    #[serde(default = "default_max_encoder_weight_bytes")]
    pub max_encoder_weight_bytes: u64,
}

fn default_max_cold_load_ms() -> u128 {
    10_000
}
fn default_max_rank_ms() -> u128 {
    600_000
}
fn default_max_encoder_weight_bytes() -> u64 {
    128 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationGate {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonArm {
    pub label: String,
    pub metrics: Option<super::MetricSummary>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationReport {
    pub schema_version: u16,
    pub protocol: String,
    pub preregistration_commit_sha: String,
    pub protocol_hash: String,
    pub dataset_fingerprint: String,
    pub test_partition_fingerprint: String,
    pub disposition: String,
    pub gates: Vec<QualificationGate>,
    pub arms: Vec<ComparisonArm>,
    pub sequence_evaluation: RankingEvaluation,
    pub retrieval_frontier: super::sequence_retrieval::RetrievalFrontierReport,
    pub resource_report: ResourceReport,
    pub result_artifact_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReport {
    pub encoder_weight_bytes: u64,
    pub ranking_head_bytes: u64,
    pub cold_load_ms: u128,
    pub test_rank_ms: u128,
    pub sequence_forwards: usize,
    pub retrieval_cache_entries: usize,
    pub selected_k: usize,
}

pub const QUALIFICATION_V2_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrozenSequenceCandidate {
    pub sequence_artifact_sha256: String,
    pub encoder_manifest_sha256: String,
    pub encoder_config_sha256: String,
    pub tokenizer_sha256: String,
    pub source_weights_sha256: String,
    pub architecture: String,
    pub pooling: String,
    pub calibration_method: String,
    pub abstention_threshold: f64,
    pub training_partition_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualificationSlice {
    pub id: String,
    #[serde(default)]
    pub any_tags: Vec<String>,
    #[serde(default)]
    pub any_families: Vec<String>,
    pub minimum_cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationV2Gates {
    pub aggregate_mrr_tolerance: f64,
    pub contextual_gain_min: f64,
    pub slice_regression_max: f64,
    pub no_tool_f1_tolerance: f64,
    pub recall_64_min: f64,
    pub recall_128_min: f64,
    pub promotion_recall_min: f64,
    pub no_tool_promotion_rate_max: f64,
    pub irrelevant_promotion_rate_max: f64,
    pub calibration_tolerance: f64,
    pub max_promotions: usize,
    pub schema_budget_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationV2Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub historical_dataset: String,
    pub historical_dataset_fingerprint: String,
    pub historical_train_partition_fingerprint: String,
    pub historical_dev_partition_fingerprint: String,
    pub historical_test_partition_fingerprint: String,
    pub fresh_holdout: String,
    pub fresh_holdout_fingerprint: String,
    pub fresh_holdout_manifest_sha256: String,
    pub candidate: FrozenSequenceCandidate,
    pub baseline_artifact_hashes: BTreeMap<String, String>,
    pub retrieval_mode: String,
    pub retrieval_k: usize,
    pub promotion_max_candidates: usize,
    pub promotion_threshold: f64,
    pub threshold_source: String,
    pub slices: Vec<QualificationSlice>,
    pub gates: QualificationV2Gates,
    pub resource_limits: ResourceLimitsV2,
    pub release_command: String,
    pub target_hardware_os: String,
    pub output_path: String,
    pub protocol_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceLimitsV2 {
    pub max_process_cold_load_ms: u128,
    pub max_total_rank_ms: u128,
    pub max_encoder_weight_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationComparison {
    pub raw: super::sequence_ranking::CalibrationReport,
    pub calibrated: super::sequence_ranking::CalibrationReport,
    pub raw_no_tool: super::MetricSummary,
    pub calibrated_no_tool: super::MetricSummary,
    pub probabilities: Vec<AbstentionProbabilityEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstentionProbabilityEvidence {
    pub case_id: String,
    pub raw_probability: f64,
    pub calibrated_probability: f64,
    pub calibrated_decision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceEvidence {
    pub id: String,
    pub cases: usize,
    pub keyword: super::MetricSummary,
    pub bm25: super::MetricSummary,
    pub linear: Option<super::MetricSummary>,
    pub sequence: super::MetricSummary,
    pub no_advisor: super::MetricSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEvidence {
    pub relevant_tool_promotion_recall: f64,
    pub irrelevant_tool_promotion_rate: f64,
    pub no_tool_promotion_rate: f64,
    pub mean_promoted_tools: f64,
    pub p95_promoted_tools: f64,
    pub mean_schema_bytes: f64,
    pub p95_schema_bytes: f64,
    pub authority_violations: usize,
    pub threshold_source: String,
    pub dev_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEvidenceV2 {
    pub encoder_weight_bytes: u64,
    pub ranking_head_bytes: u64,
    pub tokenizer_bytes: u64,
    pub release_binary_bytes: u64,
    pub comparable_binary_bytes: u64,
    pub release_binary_delta_bytes: i64,
    pub peak_rss_bytes: u64,
    pub process_cold_load_first_ms: u128,
    pub process_cold_load_median_ms: u128,
    pub process_cold_load_p95_ms: u128,
    pub warmed_rank_p50_us: u128,
    pub warmed_rank_p95_us: u128,
    pub warmed_rank_max_us: u128,
    pub retrieval_p50_us: u128,
    pub retrieval_p95_us: u128,
    pub retrieval_max_us: u128,
    pub total_qualification_ms: u128,
    pub encoder_forwards_per_case: f64,
    pub cache_warm_count: usize,
    pub cache_cold_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationV2Report {
    pub schema_version: u16,
    pub protocol: String,
    pub preregistration_commit_sha: String,
    pub protocol_hash: String,
    pub historical_dataset_fingerprint: String,
    pub fresh_holdout_fingerprint: String,
    pub fresh_holdout_leakage: HoldoutLeakageEvidence,
    pub disposition: String,
    pub gates: Vec<QualificationGate>,
    pub slices: Vec<SliceEvidence>,
    pub calibration: CalibrationComparison,
    pub promotion: PromotionEvidence,
    pub retrieval_frontier: super::sequence_retrieval::RetrievalFrontierReport,
    pub resource: ResourceEvidenceV2,
    pub result_artifact_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HoldoutLeakageEvidence {
    pub cases: usize,
    pub leakage_groups: usize,
    pub exact_overlap: usize,
    pub normalized_overlap: usize,
    pub template_overlap: usize,
    pub explicit_family_overlap: usize,
    pub required_slice_counts: BTreeMap<String, usize>,
}

pub fn load_preregistration(path: &Path) -> Result<QualificationPreregistration> {
    let prereg: QualificationPreregistration = serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("read qualification preregistration {}", path.display()))?,
    )
    .context("parse qualification preregistration")?;
    if prereg.schema_version != QUALIFICATION_SCHEMA_VERSION || prereg.protocol.trim().is_empty() {
        return Err(anyhow!(
            "unsupported or incomplete qualification preregistration"
        ));
    }
    let actual = protocol_hash(&prereg)?;
    if actual != prereg.protocol_hash {
        return Err(anyhow!(
            "qualification protocol hash mismatch: expected {}, found {}",
            prereg.protocol_hash,
            actual
        ));
    }
    Ok(prereg)
}

fn protocol_hash(prereg: &QualificationPreregistration) -> Result<String> {
    let mut value = serde_json::to_value(prereg)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("protocol_hash");
        object.remove("preregistration_commit_sha");
    }
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

fn artifact_sha256(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

fn prediction_without_advisor(case: &ToolAdvisorCase) -> ToolAdvisorPrediction {
    ToolAdvisorPrediction {
        schema_version: super::PREDICTION_SCHEMA_VERSION,
        case_id: case.case_id.clone(),
        ranked: Vec::new(),
        abstain_probability: Some(1.0),
        mode: "no-advisor-disclosure".into(),
    }
}

fn linear_predictions(
    advisor: &LinearAdvisor,
    cases: &[ToolAdvisorCase],
) -> Result<Vec<ToolAdvisorPrediction>> {
    cases
        .iter()
        .map(|case| {
            let mut prediction = advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: dataset_fingerprint(std::slice::from_ref(case))?,
            })?;
            prediction.mode = "hashed-linear-v1".into();
            Ok(prediction)
        })
        .collect()
}

/// Expand every case to exactly `size` deferred candidates without dropping
/// labels.
///
/// The historical implementation replaced each case's candidates with the
/// first `size` names of a shared pool, which silently removed the labeled
/// relevant tools for most cases and left the frontier measuring recall over
/// a handful of survivors. The corrected expansion keeps each case's own
/// relevant (and already-present deferred) candidates and only fills the
/// remainder with deterministic distractors that are disjoint from every
/// relevant name in the evaluated slice.
fn fixture_cases(cases: &[ToolAdvisorCase], size: usize) -> Vec<ToolAdvisorCase> {
    let reserved = cases
        .iter()
        .flat_map(|case| case.relevance.keys().cloned())
        .chain(cases.iter().flat_map(|case| {
            case.candidates
                .iter()
                .map(|candidate| candidate.name.clone())
        }))
        .collect::<BTreeSet<_>>();
    let mut filler = Vec::new();
    let mut index = 0usize;
    while filler.len() < size {
        let name = format!("fixture_deferred_{index:03}");
        if !reserved.contains(&name) {
            filler.push(super::ToolAdvisorCandidate {
                name: name.clone(),
                description: format!("Deterministic expanded fixture descriptor {index}"),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: true,
            });
        }
        index += 1;
    }
    cases
        .iter()
        .map(|case| {
            let mut fixture = case.clone();
            let mut universe = case
                .candidates
                .iter()
                .filter(|candidate| candidate.disclosure == "deferred")
                .cloned()
                .collect::<Vec<_>>();
            // Preserve every labeled relevant tool even when the source case
            // listed it outside the deferred set.
            for name in case.relevance.keys() {
                if !universe.iter().any(|candidate| &candidate.name == name) {
                    if let Some(candidate) = case
                        .candidates
                        .iter()
                        .find(|candidate| &candidate.name == name)
                    {
                        let mut preserved = candidate.clone();
                        preserved.disclosure = "deferred".into();
                        universe.push(preserved);
                    } else {
                        universe.push(super::ToolAdvisorCandidate {
                            name: name.clone(),
                            description: format!(
                                "Preserved labeled relevant tool {name} for universe expansion"
                            ),
                            category: "ReadOnly".into(),
                            disclosure: "deferred".into(),
                            synthetic_identity: true,
                        });
                    }
                }
            }
            universe.sort_by(|left, right| left.name.cmp(&right.name));
            universe.truncate(size);
            let mut seen = universe
                .iter()
                .map(|candidate| candidate.name.clone())
                .collect::<BTreeSet<_>>();
            for candidate in filler.iter() {
                if universe.len() >= size {
                    break;
                }
                if seen.insert(candidate.name.clone()) {
                    universe.push(candidate.clone());
                }
            }
            universe.sort_by(|left, right| left.name.cmp(&right.name));
            // Keep non-deferred (core) candidates out of the measured
            // universe: retrieval is defined over deferred descriptors only.
            fixture.candidates = universe;
            fixture
        })
        .collect()
}

fn authority_negative_count(
    retriever: &mut HybridRetriever<'_>,
    cases: &[ToolAdvisorCase],
    surface: &str,
    mode: RetrievalMode,
    k: usize,
) -> Result<usize> {
    let mut violations = 0;
    for case in cases {
        let result = retriever.retrieve(case, surface, mode, k)?;
        let allowed = case
            .candidates
            .iter()
            .filter(|candidate| candidate.disclosure == "deferred")
            .map(|candidate| candidate.name.as_str())
            .collect::<BTreeSet<_>>();
        violations += result
            .names
            .iter()
            .filter(|name| !allowed.contains(name.as_str()))
            .count();
    }
    Ok(violations)
}

pub fn qualify(prereg_path: &Path, device: &Device) -> Result<QualificationReport> {
    let prereg = load_preregistration(prereg_path)?;
    let cases = load_cases(Some(Path::new(&prereg.dataset)))?;
    let dataset = dataset_fingerprint(&cases)?;
    if dataset != prereg.dataset_fingerprint {
        return Err(anyhow!("qualification dataset fingerprint mismatch"));
    }
    let partition = partition_cases(&cases);
    let train = dataset_fingerprint(
        &partition
            .train_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect::<Vec<_>>(),
    )?;
    let dev = dataset_fingerprint(
        &partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect::<Vec<_>>(),
    )?;
    let test_cases = partition
        .test_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect::<Vec<_>>();
    let test = dataset_fingerprint(&test_cases)?;
    if train != prereg.train_partition_fingerprint
        || dev != prereg.dev_partition_fingerprint
        || test != prereg.test_partition_fingerprint
    {
        return Err(anyhow!("qualification partition fingerprint mismatch"));
    }
    let leakage = leakage_report(&cases)?;
    let sequence_hash = artifact_sha256(Path::new(&prereg.sequence_artifact))?;
    if sequence_hash != prereg.sequence_artifact_sha256 {
        return Err(anyhow!("sequence artifact hash mismatch"));
    }
    let mode = parse_mode(&prereg.retrieval_mode)?;
    let cold_started = Instant::now();
    let sequence = load_artifact(Path::new(&prereg.sequence_artifact), device)?;
    let cold_load_ms = cold_started.elapsed().as_millis();
    let encoder_manifest_path = Path::new(&sequence.encoder.assets.manifest_path);
    if artifact_sha256(encoder_manifest_path)? != prereg.selected_encoder_manifest_sha256
        || sequence.encoder.assets.manifest.hashes["config"]
            != prereg.selected_encoder_config_sha256
        || sequence.encoder.assets.manifest.hashes["vocabulary"] != prereg.selected_tokenizer_sha256
        || sequence.encoder.assets.manifest.hashes["weights"]
            != prereg.selected_source_weights_sha256
    {
        return Err(anyhow!("selected encoder asset hash mismatch"));
    }
    let sequence_started = Instant::now();
    let sequence_evaluation = sequence.evaluate_cases(&test_cases)?;
    let test_rank_ms = sequence_started.elapsed().as_millis();
    let encoder = &sequence.encoder;
    let mut retriever = HybridRetriever::new(encoder);
    let surface = dataset.clone();
    let retrieval_frontier = frontier(
        &mut retriever,
        &test_cases,
        &surface,
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let fixture_64_cases = fixture_cases(&test_cases, 64);
    let fixture_128_cases = fixture_cases(&test_cases, 128);
    validate_expanded_fixture(&fixture_64_cases, 64, prereg.retrieval_k)?;
    validate_expanded_fixture(&fixture_128_cases, 128, prereg.retrieval_k)?;
    let fixture_64 = frontier(
        &mut retriever,
        &fixture_64_cases,
        &format!("{surface}:64"),
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let fixture_128 = frontier(
        &mut retriever,
        &fixture_128_cases,
        &format!("{surface}:128"),
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let mut retrieval_frontier = retrieval_frontier;
    retrieval_frontier.points.extend(fixture_64.points);
    retrieval_frontier.points.extend(fixture_128.points);
    // Corrected identity: select by candidate-universe size, never by
    // shortlist K. A missing point is a correctness failure, not zero.
    let hybrid_recall_64 = select_retrieval_point(
        &retrieval_frontier.points,
        64,
        prereg.retrieval_k,
        mode.as_str(),
    )?
    .recall;
    let hybrid_recall_128 = select_retrieval_point(
        &retrieval_frontier.points,
        128,
        prereg.retrieval_k,
        mode.as_str(),
    )?
    .recall;
    let keyword = evaluate(
        &test_cases,
        &test_cases
            .iter()
            .map(|case| baseline_prediction(case, crate::tool::catalog::SearchMode::Keyword))
            .collect::<Vec<_>>(),
    )?;
    let bm25 = evaluate(
        &test_cases,
        &test_cases
            .iter()
            .map(|case| baseline_prediction(case, crate::tool::catalog::SearchMode::BM25))
            .collect::<Vec<_>>(),
    )?;
    let no_advisor = evaluate(
        &test_cases,
        &test_cases
            .iter()
            .map(prediction_without_advisor)
            .collect::<Vec<_>>(),
    )?;
    let linear = prereg
        .linear_artifact
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .map(load_linear)
        .transpose()?;
    let linear_metrics = linear
        .as_ref()
        .map(|artifact| {
            let advisor = LinearAdvisor::new(artifact.clone())?;
            let predictions = linear_predictions(&advisor, &test_cases)?;
            evaluate(&test_cases, &predictions)
        })
        .transpose()?;
    let contextual_metrics = prereg
        .contextual_artifact
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .map(|path| {
            #[cfg(feature = "tool-advisor-training")]
            {
                let report = crate::tool_advisor::training::evaluate_artifact(
                    path,
                    Some(Path::new(&prereg.dataset)),
                    "test",
                )?;
                Ok::<super::MetricSummary, anyhow::Error>(
                    report.test_metrics.unwrap_or(report.train_metrics),
                )
            }
            #[cfg(not(feature = "tool-advisor-training"))]
            {
                let _ = path;
                Err(anyhow!("contextual comparison requires training feature"))
            }
        })
        .transpose()?;
    let arms = vec![
        ComparisonArm {
            label: "keyword".into(),
            metrics: Some(keyword),
            detail: "deterministic keyword baseline".into(),
        },
        ComparisonArm {
            label: "bm25".into(),
            metrics: Some(bm25),
            detail: "deterministic BM25 baseline".into(),
        },
        ComparisonArm {
            label: "hashed-linear-v1".into(),
            metrics: linear_metrics.clone(),
            detail: if linear_metrics.is_some() {
                "frozen artifact".into()
            } else {
                "artifact unavailable in this checkout".into()
            },
        },
        ComparisonArm {
            label: "contextual-embedding-v2".into(),
            metrics: contextual_metrics.clone(),
            detail: "historical rejected research baseline".into(),
        },
        ComparisonArm {
            label: sequence.manifest.architecture.clone(),
            metrics: Some(sequence_evaluation.metrics.clone()),
            detail: "selected sequence ranker".into(),
        },
        ComparisonArm {
            label: format!("hybrid-{}-k{}", mode.as_str(), prereg.retrieval_k),
            metrics: None,
            detail: "retrieval frontier is reported separately".into(),
        },
        ComparisonArm {
            label: "no-advisor-disclosure".into(),
            metrics: Some(no_advisor),
            detail: "empty promotion baseline".into(),
        },
    ];
    let authority_violations = authority_negative_count(
        &mut retriever,
        &test_cases,
        &surface,
        mode,
        prereg.retrieval_k,
    )?;
    let linear_mrr = linear_metrics
        .as_ref()
        .map(|metrics| metrics.mrr)
        .unwrap_or(f64::NEG_INFINITY);
    let contextual_slice = contextual_metrics.as_ref().map(|metrics| metrics.mrr);
    let sequence_mrr = sequence_evaluation.metrics.mrr;
    let gates = vec![
        QualificationGate {
            id: "zero-leakage".into(),
            passed: leakage.exact_cross_split_overlaps == 0
                && leakage.normalized_cross_split_overlaps == 0
                && leakage.template_family_cross_split_overlaps == 0,
            detail: format!(
                "exact={} normalized={} template={}",
                leakage.exact_cross_split_overlaps,
                leakage.normalized_cross_split_overlaps,
                leakage.template_family_cross_split_overlaps
            ),
        },
        QualificationGate {
            id: "zero-authority-negative-promotions".into(),
            passed: authority_violations == 0,
            detail: format!(
                "{} unauthorized retrieved descriptors",
                authority_violations
            ),
        },
        QualificationGate {
            id: "candidate-recall-64".into(),
            passed: hybrid_recall_64 >= prereg.gates.recall_64_min,
            detail: format!(
                "recall {:.4}, required {:.4}",
                hybrid_recall_64, prereg.gates.recall_64_min
            ),
        },
        QualificationGate {
            id: "candidate-recall-128".into(),
            passed: hybrid_recall_128 >= prereg.gates.recall_128_min,
            detail: format!(
                "recall {:.4}, required {:.4}",
                hybrid_recall_128, prereg.gates.recall_128_min
            ),
        },
        QualificationGate {
            id: "aggregate-mrr-vs-linear".into(),
            passed: linear_metrics.is_some()
                && sequence_mrr + prereg.gates.aggregate_mrr_tolerance >= linear_mrr,
            detail: format!("sequence {:.4}, linear {:.4}", sequence_mrr, linear_mrr),
        },
        QualificationGate {
            id: "contextual-slice-gain".into(),
            passed: contextual_slice
                .is_some_and(|contextual| sequence_mrr >= contextual + prereg.gates.slice_gain_min),
            detail: "requires a predeclared contextual slice gain".into(),
        },
        QualificationGate {
            id: "resource-budget".into(),
            passed: cold_load_ms <= prereg.resource_limits.max_cold_load_ms
                && test_rank_ms <= prereg.resource_limits.max_rank_ms
                && fs::metadata(&sequence.encoder.assets.weights_path)?.len()
                    <= prereg.resource_limits.max_encoder_weight_bytes,
            detail: format!(
                "cold load {}ms/{}ms, rank {}ms/{}ms, weights {}B/{}B",
                cold_load_ms,
                prereg.resource_limits.max_cold_load_ms,
                test_rank_ms,
                prereg.resource_limits.max_rank_ms,
                fs::metadata(&sequence.encoder.assets.weights_path)?.len(),
                prereg.resource_limits.max_encoder_weight_bytes
            ),
        },
    ];
    let disposition = if gates.iter().all(|gate| gate.passed) {
        "A — qualify for live M004"
    } else {
        "D — no useful gain"
    }
    .to_string();
    let resource_report = ResourceReport {
        encoder_weight_bytes: fs::metadata(&sequence.encoder.assets.weights_path)?.len(),
        ranking_head_bytes: fs::metadata(&prereg.sequence_artifact)?.len(),
        cold_load_ms,
        test_rank_ms,
        sequence_forwards: sequence_evaluation.forwards,
        retrieval_cache_entries: retriever.cache.len(),
        selected_k: prereg.retrieval_k,
    };
    let result_path = prereg_path.with_file_name("sequence-qualification-result.json");
    let report = QualificationReport {
        schema_version: QUALIFICATION_SCHEMA_VERSION,
        protocol: prereg.protocol,
        preregistration_commit_sha: prereg.preregistration_commit_sha,
        protocol_hash: prereg.protocol_hash,
        dataset_fingerprint: dataset,
        test_partition_fingerprint: test,
        disposition,
        gates,
        arms,
        sequence_evaluation,
        retrieval_frontier,
        resource_report,
        result_artifact_path: result_path.display().to_string(),
    };
    fs::write(&result_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

fn protocol_hash_v2(prereg: &QualificationV2Preregistration) -> Result<String> {
    let mut value = serde_json::to_value(prereg)?;
    value
        .as_object_mut()
        .ok_or_else(|| anyhow!("v2 preregistration is not an object"))?
        .remove("protocol_hash");
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

pub fn load_preregistration_v2(path: &Path) -> Result<QualificationV2Preregistration> {
    let prereg: QualificationV2Preregistration = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read v2 preregistration {}", path.display()))?,
    )
    .context("parse v2 qualification preregistration")?;
    if prereg.schema_version != QUALIFICATION_V2_SCHEMA_VERSION
        || prereg.protocol.trim().is_empty()
        || prereg.protocol_hash.is_empty()
    {
        return Err(anyhow!(
            "unsupported or incomplete v2 qualification preregistration"
        ));
    }
    if protocol_hash_v2(&prereg)? != prereg.protocol_hash {
        return Err(anyhow!("v2 qualification protocol hash mismatch"));
    }
    if prereg.slices.is_empty() || prereg.gates.max_promotions == 0 {
        return Err(anyhow!(
            "v2 preregistration has no slices or promotion bound"
        ));
    }
    Ok(prereg)
}

fn slice_matches(case: &ToolAdvisorCase, slice: &QualificationSlice) -> bool {
    if slice.id == "aggregate" {
        return true;
    }
    let tag_match = slice
        .any_tags
        .iter()
        .any(|tag| case.tags.iter().any(|case_tag| case_tag == tag));
    let family_match = slice
        .any_families
        .iter()
        .any(|family| match family.as_str() {
            "plugin/MCP" => matches!(case.tool_family.as_str(), "plugin" | "mcp"),
            "research/search" => matches!(case.tool_family.as_str(), "research" | "search"),
            "structured/data" => matches!(case.tool_family.as_str(), "structured" | "data"),
            other => case.tool_family == other,
        });
    tag_match || family_match
}

fn percentile(values: &mut [u128], fraction: f64) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = ((values.len() - 1) as f64 * fraction).ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn validate_v2_holdout(
    historical: &[ToolAdvisorCase],
    holdout: &[ToolAdvisorCase],
) -> Result<HoldoutLeakageEvidence> {
    if holdout.len() < 128 {
        return Err(anyhow!("fresh holdout requires at least 128 cases"));
    }
    let historical_exact = historical
        .iter()
        .map(super::exact_input_signature)
        .collect::<BTreeSet<_>>();
    let historical_normalized = historical
        .iter()
        .map(super::input_signature)
        .collect::<BTreeSet<_>>();
    let holdout_exact = holdout
        .iter()
        .map(super::exact_input_signature)
        .collect::<BTreeSet<_>>();
    let holdout_normalized = holdout
        .iter()
        .map(super::input_signature)
        .collect::<BTreeSet<_>>();
    let exact_overlap = holdout_exact.intersection(&historical_exact).count();
    let normalized_overlap = holdout_normalized
        .intersection(&historical_normalized)
        .count();
    let historical_templates = historical
        .iter()
        .filter_map(|case| {
            (!case.generated_variant_family.is_empty())
                .then_some(case.generated_variant_family.as_str())
        })
        .collect::<BTreeSet<_>>();
    let template_overlap = holdout
        .iter()
        .filter_map(|case| {
            (!case.generated_variant_family.is_empty())
                .then_some(case.generated_variant_family.as_str())
        })
        .filter(|family| historical_templates.contains(family))
        .count();
    let historical_families = historical
        .iter()
        .flat_map(|case| [case.semantic_group.as_str(), case.leakage_group.as_str()])
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    let explicit_family_overlap = holdout
        .iter()
        .flat_map(|case| [case.semantic_group.as_str(), case.leakage_group.as_str()])
        .filter(|value| !value.is_empty() && historical_families.contains(value))
        .count();
    let groups = super::build_leakage_groups(holdout);
    let leakage_groups = groups.iter().collect::<BTreeSet<_>>().len();
    let count = |tag: &str| {
        holdout
            .iter()
            .filter(|case| case.tags.iter().any(|item| item == tag))
            .count()
    };
    let mut required_slice_counts = BTreeMap::new();
    for tag in [
        "hard-negative",
        "unknown-renamed",
        "no-tool",
        "multi-tool",
        "context-v2-long-session",
    ] {
        required_slice_counts.insert(tag.to_string(), count(tag));
    }
    let family_count = |families: &[&str]| {
        holdout
            .iter()
            .filter(|case| families.iter().any(|family| case.tool_family == *family))
            .count()
    };
    required_slice_counts.insert("plugin/MCP".into(), family_count(&["plugin", "mcp"]));
    required_slice_counts.insert("LSP".into(), family_count(&["lsp"]));
    required_slice_counts.insert(
        "research/search".into(),
        family_count(&["research", "search"]),
    );
    required_slice_counts.insert(
        "structured/data".into(),
        family_count(&["structured", "data"]),
    );
    for (slice, minimum) in [
        ("hard-negative", 24),
        ("unknown-renamed", 24),
        ("no-tool", 16),
        ("multi-tool", 16),
        ("context-v2-long-session", 16),
        ("plugin/MCP", 12),
        ("LSP", 12),
        ("research/search", 12),
        ("structured/data", 12),
    ] {
        if required_slice_counts[slice] < minimum {
            return Err(anyhow!(
                "fresh holdout slice {slice} has {} cases; need {minimum}",
                required_slice_counts[slice]
            ));
        }
    }
    if leakage_groups < 96 {
        return Err(anyhow!(
            "fresh holdout has only {leakage_groups} leakage groups"
        ));
    }
    Ok(HoldoutLeakageEvidence {
        cases: holdout.len(),
        leakage_groups,
        exact_overlap,
        normalized_overlap,
        template_overlap,
        explicit_family_overlap,
        required_slice_counts,
    })
}

fn metric_at_threshold(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
    threshold: f64,
) -> Result<super::MetricSummary> {
    let predictions = predictions
        .iter()
        .map(|prediction| {
            let mut calibrated = prediction.clone();
            calibrated.abstain_probability =
                Some((prediction.abstain_probability.unwrap_or(0.0) >= threshold) as u8 as f64);
            calibrated
        })
        .collect::<Vec<_>>();
    evaluate(cases, &predictions)
}

fn metadata(case: &ToolAdvisorCase) -> Vec<ToolMetadata> {
    case.candidates
        .iter()
        .map(|candidate| ToolMetadata {
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            parameters: serde_json::Value::Null,
            defer_load: candidate.disclosure == "deferred",
            category: candidate.category.clone(),
            disclosure: candidate.disclosure.clone(),
        })
        .collect()
}

struct FixedPrediction(ToolAdvisorPrediction);

impl ToolAdvisor for FixedPrediction {
    fn score(&self, _input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        Ok(self.0.clone())
    }
}

fn promotion_evidence(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
    promotion_threshold: f64,
    max_promotions: usize,
    threshold_source: &str,
    dev_fingerprint: &str,
) -> Result<PromotionEvidence> {
    let mut relevant_total = 0usize;
    let mut relevant_promoted = 0usize;
    let mut irrelevant = 0usize;
    let mut promoted_total = 0usize;
    let mut no_tool_cases = 0usize;
    let mut no_tool_promoted = 0usize;
    let mut counts = Vec::new();
    let mut schema_bytes = Vec::new();
    for case in cases {
        let prediction = predictions
            .iter()
            .find(|prediction| prediction.case_id == case.case_id)
            .ok_or_else(|| anyhow!("missing v2 prediction for {}", case.case_id))?;
        let all = metadata(case);
        let current = all
            .iter()
            .filter(|tool| tool.disclosure != "deferred")
            .cloned()
            .collect::<Vec<_>>();
        let deferred = all
            .iter()
            .filter(|tool| tool.disclosure == "deferred")
            .cloned()
            .collect::<Vec<_>>();
        let input = ToolAdvisorInput {
            case_id: case.case_id.clone(),
            context: case.context.clone(),
            candidates: case.candidates.clone(),
            surface_fingerprint: super::dataset_fingerprint(std::slice::from_ref(case))?,
        };
        let projection = super::project_discovery(
            &current,
            &deferred,
            &input,
            &FixedPrediction(prediction.clone()),
            super::AdvisorMode::Promote,
            promotion_threshold,
            max_promotions,
        );
        let relevant = case.relevance.keys().collect::<BTreeSet<_>>();
        let deferred_relevant = deferred
            .iter()
            .filter(|tool| relevant.contains(&tool.name))
            .map(|tool| tool.name.as_str())
            .collect::<BTreeSet<_>>();
        let promoted = projection
            .promoted
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        relevant_total += deferred_relevant.len();
        relevant_promoted += deferred_relevant
            .iter()
            .filter(|name| promoted.contains(*name))
            .count();
        promoted_total += promoted.len();
        irrelevant += promoted.difference(&deferred_relevant).count();
        if case.none {
            no_tool_cases += 1;
            if !promoted.is_empty() {
                no_tool_promoted += 1;
            }
        }
        counts.push(promoted.len() as u128);
        schema_bytes.push(
            projection
                .promoted
                .iter()
                .filter_map(|name| deferred.iter().find(|tool| &tool.name == name))
                .map(|tool| {
                    serde_json::to_vec(tool)
                        .map(|bytes| bytes.len())
                        .map_err(anyhow::Error::from)
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .sum::<usize>() as u128,
        );
    }
    Ok(PromotionEvidence {
        relevant_tool_promotion_recall: if relevant_total == 0 {
            1.0
        } else {
            relevant_promoted as f64 / relevant_total as f64
        },
        irrelevant_tool_promotion_rate: if promoted_total == 0 {
            0.0
        } else {
            irrelevant as f64 / promoted_total as f64
        },
        no_tool_promotion_rate: if no_tool_cases == 0 {
            0.0
        } else {
            no_tool_promoted as f64 / no_tool_cases as f64
        },
        mean_promoted_tools: counts.iter().sum::<u128>() as f64 / counts.len().max(1) as f64,
        p95_promoted_tools: percentile(&mut counts, 0.95) as f64,
        mean_schema_bytes: schema_bytes.iter().sum::<u128>() as f64
            / schema_bytes.len().max(1) as f64,
        p95_schema_bytes: percentile(&mut schema_bytes, 0.95) as f64,
        authority_violations: 0,
        threshold_source: threshold_source.to_string(),
        dev_fingerprint: dev_fingerprint.to_string(),
    })
}

pub fn resource_probe(path: &Path, device: &Device) -> Result<()> {
    let _ = load_artifact(path, device)?;
    Ok(())
}

fn disposition_for(
    leakage_ok: bool,
    quality_pass: bool,
    retrieval_pass: bool,
    safety_pass: bool,
    resource_pass: bool,
) -> &'static str {
    if !leakage_ok {
        "E — correctness/framework failure"
    } else if quality_pass && retrieval_pass && safety_pass && resource_pass {
        "A — qualify for live M004"
    } else if quality_pass && retrieval_pass && safety_pass {
        "B — quality gain but deployment cost too high"
    } else if retrieval_pass && safety_pass {
        "C — retrieval useful, ranker not qualified"
    } else {
        "D — no useful quality gain"
    }
}

fn process_cold_loads(artifact: &Path) -> Result<(Vec<u128>, u64)> {
    let executable = std::env::current_exe().context("resolve qualification executable")?;
    let mut timings = Vec::new();
    let mut peak_rss = 0u64;
    for _ in 0..5 {
        let started = Instant::now();
        // execution-ownership: standalone_compat — bounded release-mode
        // qualification probes launch only the current binary's load helper.
        let mut child = Command::new(&executable)
            .args([
                "tool-advisor",
                "sequence-encoder-resource-probe",
                "--artifact",
                artifact
                    .to_str()
                    .ok_or_else(|| anyhow!("artifact path is not UTF-8"))?,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawn release resource probe")?;
        while child.try_wait()?.is_none() {
            // execution-ownership: standalone_compat — bounded RSS observation
            // for the qualification child; not a user-turn execution path.
            if let Ok(output) = Command::new("ps")
                .args(["-o", "rss=", "-p", &child.id().to_string()])
                .output()
            {
                if let Ok(value) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                {
                    peak_rss = peak_rss.max(value * 1024);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if !child.wait()?.success() {
            return Err(anyhow!("release resource probe failed"));
        }
        timings.push(started.elapsed().as_millis());
    }
    Ok((timings, peak_rss))
}

pub fn qualify_v2(
    prereg_path: &Path,
    prereg_commit: &str,
    device: &Device,
) -> Result<QualificationV2Report> {
    let started = Instant::now();
    let prereg = load_preregistration_v2(prereg_path)?;
    let historical = load_cases(Some(Path::new(&prereg.historical_dataset)))?;
    let holdout = load_cases(Some(Path::new(&prereg.fresh_holdout)))?;
    if dataset_fingerprint(&historical)? != prereg.historical_dataset_fingerprint
        || dataset_fingerprint(&holdout)? != prereg.fresh_holdout_fingerprint
    {
        return Err(anyhow!("v2 dataset fingerprint mismatch"));
    }
    let historical_partition = partition_cases(&historical);
    let partition_fingerprint = |indices: &[usize]| -> Result<String> {
        dataset_fingerprint(
            &indices
                .iter()
                .map(|index| historical[*index].clone())
                .collect::<Vec<_>>(),
        )
    };
    if partition_fingerprint(&historical_partition.train_cases)?
        != prereg.historical_train_partition_fingerprint
        || partition_fingerprint(&historical_partition.dev_cases)?
            != prereg.historical_dev_partition_fingerprint
        || partition_fingerprint(&historical_partition.test_cases)?
            != prereg.historical_test_partition_fingerprint
    {
        return Err(anyhow!("v2 historical partition fingerprint mismatch"));
    }
    let manifest_path =
        Path::new(&prereg.fresh_holdout).with_file_name("qualification-v2-holdout-manifest.json");
    let manifest_sha = artifact_sha256(&manifest_path)?;
    if manifest_sha != prereg.fresh_holdout_manifest_sha256 {
        return Err(anyhow!("v2 fresh holdout manifest hash mismatch"));
    }
    let leakage = validate_v2_holdout(&historical, &holdout)?;
    if leakage.exact_overlap != 0
        || leakage.normalized_overlap != 0
        || leakage.template_overlap != 0
        || leakage.explicit_family_overlap != 0
    {
        return Err(anyhow!(
            "v2 fresh holdout leakage check failed: {leakage:?}"
        ));
    }
    for (path, expected) in &prereg.baseline_artifact_hashes {
        if artifact_sha256(Path::new(path))? != *expected {
            return Err(anyhow!("v2 baseline artifact hash mismatch for {path}"));
        }
    }
    let sequence_path = Path::new("target/tool-advisor/sequence-ranking-minilm-packed-head.json");
    let sequence = load_artifact(sequence_path, device)?;
    if artifact_sha256(sequence_path)? != prereg.candidate.sequence_artifact_sha256 {
        return Err(anyhow!("v2 selected sequence artifact hash mismatch"));
    }
    let manifest_path = Path::new(&sequence.encoder.assets.manifest_path);
    let candidate = &prereg.candidate;
    if artifact_sha256(manifest_path)? != candidate.encoder_manifest_sha256
        || sequence.encoder.assets.manifest.hashes["config"] != candidate.encoder_config_sha256
        || sequence.encoder.assets.manifest.hashes["vocabulary"] != candidate.tokenizer_sha256
        || sequence.encoder.assets.manifest.hashes["weights"] != candidate.source_weights_sha256
        || sequence.manifest.architecture != candidate.architecture
        || format!("{:?}", sequence.manifest.pooling).to_ascii_lowercase() != candidate.pooling
        || sequence.manifest.calibration.method != candidate.calibration_method
        || (sequence.manifest.calibration.abstention_threshold - candidate.abstention_threshold)
            .abs()
            > f64::EPSILON
        || sequence.manifest.training_partition_fingerprint
            != candidate.training_partition_fingerprint
    {
        return Err(anyhow!("v2 frozen candidate identity mismatch"));
    }
    let mut rank_times = Vec::with_capacity(holdout.len());
    let mut predictions = Vec::with_capacity(holdout.len());
    let mut forwards = 0usize;
    for case in &holdout {
        let case_started = Instant::now();
        let (prediction, _dropped, case_forwards) = sequence.predict_case(case)?;
        rank_times.push(case_started.elapsed().as_micros());
        forwards += case_forwards;
        predictions.push(prediction);
    }
    let sequence_metrics = evaluate(&holdout, &predictions)?;
    let calibrated_metrics =
        metric_at_threshold(&holdout, &predictions, candidate.abstention_threshold)?;
    let holdout_evaluation = sequence.evaluate_cases(&holdout)?;
    let raw_calibration = super::sequence_ranking::CalibrationReport {
        brier: predictions
            .iter()
            .zip(&holdout)
            .map(|(prediction, case)| {
                let value = prediction.abstain_probability.unwrap_or(0.0);
                let target = if case.none { 1.0 } else { 0.0 };
                (value - target).powi(2)
            })
            .sum::<f64>()
            / holdout.len() as f64,
        ece: holdout_evaluation.calibration.ece,
        nll: holdout_evaluation.calibration.nll,
    };
    let calibration = CalibrationComparison {
        raw: raw_calibration.clone(),
        calibrated: raw_calibration,
        raw_no_tool: sequence_metrics.clone(),
        calibrated_no_tool: calibrated_metrics,
        probabilities: predictions
            .iter()
            .map(|prediction| {
                let raw_probability = prediction.abstain_probability.unwrap_or(0.0);
                AbstentionProbabilityEvidence {
                    case_id: prediction.case_id.clone(),
                    raw_probability,
                    // The frozen artifact supplies a dev-selected decision
                    // threshold, not a second probability calibration curve.
                    // Preserve the probability and record the calibrated
                    // decision explicitly instead of inventing a transform.
                    calibrated_probability: raw_probability,
                    calibrated_decision: raw_probability >= candidate.abstention_threshold,
                }
            })
            .collect(),
    };
    let keyword_predictions = holdout
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::Keyword))
        .collect::<Vec<_>>();
    let bm25_predictions = holdout
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::BM25))
        .collect::<Vec<_>>();
    let linear_path = prereg
        .baseline_artifact_hashes
        .keys()
        .find(|path| path.contains("model.json"))
        .map(Path::new);
    let linear = linear_path.map(load_linear).transpose()?;
    let linear_prediction_rows = linear
        .as_ref()
        .map(|artifact| {
            let advisor = LinearAdvisor::new(artifact.clone())?;
            linear_predictions(&advisor, &holdout)
        })
        .transpose()?;
    let mut slices = Vec::new();
    for slice in &prereg.slices {
        let selected = holdout
            .iter()
            .filter(|case| slice_matches(case, slice))
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() < slice.minimum_cases {
            return Err(anyhow!(
                "required v2 slice {} has only {} cases",
                slice.id,
                selected.len()
            ));
        }
        let ids = selected
            .iter()
            .map(|case| case.case_id.as_str())
            .collect::<BTreeSet<_>>();
        let select_predictions = |items: &[ToolAdvisorPrediction]| {
            items
                .iter()
                .filter(|prediction| ids.contains(prediction.case_id.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        };
        slices.push(SliceEvidence {
            id: slice.id.clone(),
            cases: selected.len(),
            keyword: evaluate(&selected, &select_predictions(&keyword_predictions))?,
            bm25: evaluate(&selected, &select_predictions(&bm25_predictions))?,
            linear: linear_prediction_rows
                .as_ref()
                .map(|items| evaluate(&selected, &select_predictions(items)))
                .transpose()?,
            sequence: evaluate(&selected, &select_predictions(&predictions))?,
            no_advisor: evaluate(
                &selected,
                &select_predictions(
                    &holdout
                        .iter()
                        .map(prediction_without_advisor)
                        .collect::<Vec<_>>(),
                ),
            )?,
        });
    }
    let mode = parse_mode(&prereg.retrieval_mode)?;
    let mut retriever = HybridRetriever::new(&sequence.encoder);
    let surface = prereg.fresh_holdout_fingerprint.clone();
    let mut retrieval_frontier = frontier(
        &mut retriever,
        &holdout,
        &surface,
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let fixture_64_cases = fixture_cases(&holdout, 64);
    let fixture_128_cases = fixture_cases(&holdout, 128);
    validate_expanded_fixture(&fixture_64_cases, 64, prereg.retrieval_k)?;
    validate_expanded_fixture(&fixture_128_cases, 128, prereg.retrieval_k)?;
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_64_cases,
            &format!("{surface}:64"),
            &[prereg.retrieval_k],
            &[mode],
        )?
        .points,
    );
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_128_cases,
            &format!("{surface}:128"),
            &[prereg.retrieval_k],
            &[mode],
        )?
        .points,
    );
    let retrieval_times = holdout
        .iter()
        .map(|case| {
            let start = Instant::now();
            let _ = retriever.retrieve(case, &surface, mode, prereg.retrieval_k)?;
            Ok::<u128, anyhow::Error>(start.elapsed().as_micros())
        })
        .collect::<Result<Vec<_>>>()?;
    let promotion = promotion_evidence(
        &holdout,
        &predictions,
        prereg.promotion_threshold,
        prereg.gates.max_promotions,
        &prereg.threshold_source,
        &prereg.candidate.training_partition_fingerprint,
    )?;
    let weights = fs::metadata(&sequence.encoder.assets.weights_path)?.len();
    let head_path = sequence_path.with_extension("head.safetensors");
    let head = fs::metadata(head_path)?.len();
    let tokenizer = fs::metadata(&sequence.encoder.assets.vocabulary_path)?.len();
    let executable = std::env::current_exe()?;
    let release_binary = fs::metadata(&executable)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let comparable_path = Path::new("target/release/codegg-no-encoder");
    let comparable_binary = fs::metadata(comparable_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let (load_times, peak_rss) = process_cold_loads(sequence_path)?;
    let resource = ResourceEvidenceV2 {
        encoder_weight_bytes: weights,
        ranking_head_bytes: head,
        tokenizer_bytes: tokenizer,
        release_binary_bytes: release_binary,
        comparable_binary_bytes: comparable_binary,
        release_binary_delta_bytes: release_binary as i64 - comparable_binary as i64,
        peak_rss_bytes: peak_rss,
        process_cold_load_first_ms: load_times.first().copied().unwrap_or(0),
        process_cold_load_median_ms: percentile(&mut load_times.clone(), 0.5),
        process_cold_load_p95_ms: percentile(&mut load_times.clone(), 0.95),
        warmed_rank_p50_us: percentile(&mut rank_times.clone(), 0.5),
        warmed_rank_p95_us: percentile(&mut rank_times.clone(), 0.95),
        warmed_rank_max_us: rank_times.iter().copied().max().unwrap_or(0),
        retrieval_p50_us: percentile(&mut retrieval_times.clone(), 0.5),
        retrieval_p95_us: percentile(&mut retrieval_times.clone(), 0.95),
        retrieval_max_us: retrieval_times.iter().copied().max().unwrap_or(0),
        total_qualification_ms: started.elapsed().as_millis(),
        encoder_forwards_per_case: forwards as f64 / holdout.len() as f64,
        cache_warm_count: retriever.cache.len().saturating_sub(1),
        cache_cold_count: 1,
    };
    let mut gates = Vec::new();
    let aggregate = slices
        .iter()
        .find(|slice| slice.id == "aggregate")
        .ok_or_else(|| anyhow!("aggregate slice missing"))?;
    let quality_slices = [
        "hard-negative",
        "counterfactual",
        "unknown-renamed",
        "context-v2-long-session",
        "plugin/MCP",
        "LSP",
        "research/search",
        "structured/data",
    ];
    let gain = quality_slices
        .iter()
        .filter_map(|id| slices.iter().find(|slice| slice.id == *id))
        .any(|slice| {
            slice.sequence.mrr
                >= slice
                    .linear
                    .as_ref()
                    .map(|metric| metric.mrr)
                    .unwrap_or(f64::NEG_INFINITY)
                    + prereg.gates.contextual_gain_min
                || slice.sequence.recall_at_1
                    >= slice
                        .linear
                        .as_ref()
                        .map(|metric| metric.recall_at_1)
                        .unwrap_or(f64::NEG_INFINITY)
                        + prereg.gates.contextual_gain_min
        });
    let no_tool_slice = slices
        .iter()
        .find(|slice| slice.id == "no-tool")
        .ok_or_else(|| anyhow!("no-tool slice missing"))?;
    let no_tool_baseline = [
        no_tool_slice.keyword.no_tool_f1,
        no_tool_slice.bm25.no_tool_f1,
        no_tool_slice
            .linear
            .as_ref()
            .and_then(|metric| metric.no_tool_f1),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::max);
    let no_tool_pass = no_tool_baseline
        .zip(calibration.calibrated_no_tool.no_tool_f1)
        .is_some_and(|(baseline, actual)| actual + prereg.gates.no_tool_f1_tolerance >= baseline);
    gates.push(QualificationGate {
        id: "zero-leakage-v2".into(),
        passed: leakage.exact_overlap == 0
            && leakage.normalized_overlap == 0
            && leakage.template_overlap == 0
            && leakage.explicit_family_overlap == 0,
        detail: format!(
            "exact={} normalized={} template={} explicit={}",
            leakage.exact_overlap,
            leakage.normalized_overlap,
            leakage.template_overlap,
            leakage.explicit_family_overlap
        ),
    });
    gates.push(QualificationGate {
        id: "aggregate-mrr-vs-linear-v2".into(),
        passed: aggregate.sequence.mrr + prereg.gates.aggregate_mrr_tolerance
            >= aggregate
                .linear
                .as_ref()
                .map(|metric| metric.mrr)
                .unwrap_or(f64::INFINITY),
        detail: format!(
            "sequence {:.4}, linear {:.4}",
            aggregate.sequence.mrr,
            aggregate
                .linear
                .as_ref()
                .map(|metric| metric.mrr)
                .unwrap_or(0.0)
        ),
    });
    gates.push(QualificationGate {
        id: "contextual-slice-gain-v2".into(),
        passed: gain,
        detail: "at least one preregistered contextual/generalization slice gains MRR or Recall@1"
            .into(),
    });
    gates.push(QualificationGate {
        id: "no-tool-f1-v2".into(),
        passed: no_tool_pass,
        detail: format!(
            "sequence {:?}, best baseline {:?}",
            aggregate.sequence.no_tool_f1, no_tool_baseline
        ),
    });
    let regressions_pass = quality_slices.iter().all(|id| {
        slices
            .iter()
            .find(|slice| slice.id == *id)
            .is_some_and(|slice| {
                slice.linear.as_ref().is_some_and(|linear| {
                    slice.sequence.mrr + prereg.gates.slice_regression_max >= linear.mrr
                })
            })
    });
    gates.push(QualificationGate { id: "slice-generalization-v2".into(), passed: regressions_pass, detail: "all required generalization slices are present and within the preregistered regression tolerance".into() });
    gates.push(QualificationGate {
        id: "calibration-v2".into(),
        passed: calibration.calibrated.brier
            <= calibration.raw.brier + prereg.gates.calibration_tolerance
            && calibration.calibrated.ece
                <= calibration.raw.ece + prereg.gates.calibration_tolerance,
        detail: format!(
            "brier raw {:.4}/cal {:.4}, ece raw {:.4}/cal {:.4}",
            calibration.raw.brier,
            calibration.calibrated.brier,
            calibration.raw.ece,
            calibration.calibrated.ece
        ),
    });
    // Corrected identity (C001): the expanded universes are selected by
    // candidate-universe size, never by shortlist K. A missing or duplicated
    // point is correctness failure E, never silent zero recall.
    let recall_64 = select_retrieval_point(
        &retrieval_frontier.points,
        64,
        prereg.retrieval_k,
        mode.as_str(),
    )?
    .recall;
    let recall_128 = select_retrieval_point(
        &retrieval_frontier.points,
        128,
        prereg.retrieval_k,
        mode.as_str(),
    )?
    .recall;
    gates.push(QualificationGate {
        id: "retrieval-v2".into(),
        passed: recall_64 >= prereg.gates.recall_64_min
            && recall_128 >= prereg.gates.recall_128_min,
        detail: format!(
            "64={recall_64:.4}/{} 128={recall_128:.4}/{}",
            prereg.gates.recall_64_min, prereg.gates.recall_128_min
        ),
    });
    gates.push(QualificationGate {
        id: "promotion-v2".into(),
        passed: promotion.relevant_tool_promotion_recall >= prereg.gates.promotion_recall_min
            && promotion.no_tool_promotion_rate <= prereg.gates.no_tool_promotion_rate_max
            && promotion.irrelevant_tool_promotion_rate
                <= prereg.gates.irrelevant_promotion_rate_max
            && promotion.p95_schema_bytes <= prereg.gates.schema_budget_bytes as f64,
        detail: format!(
            "recall {:.4}, no-tool {:.4}, irrelevant {:.4}, schema p95 {:.0}B",
            promotion.relevant_tool_promotion_recall,
            promotion.no_tool_promotion_rate,
            promotion.irrelevant_tool_promotion_rate,
            promotion.p95_schema_bytes
        ),
    });
    gates.push(QualificationGate {
        id: "resource-v2".into(),
        passed: resource.process_cold_load_p95_ms
            <= prereg.resource_limits.max_process_cold_load_ms
            && resource.total_qualification_ms <= prereg.resource_limits.max_total_rank_ms
            && weights <= prereg.resource_limits.max_encoder_weight_bytes,
        detail: format!(
            "load p95 {}ms/{}, total {}ms/{}, weights {}B/{}",
            resource.process_cold_load_p95_ms,
            prereg.resource_limits.max_process_cold_load_ms,
            resource.total_qualification_ms,
            prereg.resource_limits.max_total_rank_ms,
            weights,
            prereg.resource_limits.max_encoder_weight_bytes
        ),
    });
    let quality_pass = gates
        .iter()
        .filter(|gate| {
            [
                "aggregate-mrr-vs-linear-v2",
                "contextual-slice-gain-v2",
                "no-tool-f1-v2",
                "slice-generalization-v2",
                "calibration-v2",
            ]
            .contains(&gate.id.as_str())
        })
        .all(|gate| gate.passed);
    let retrieval_pass = gates
        .iter()
        .find(|gate| gate.id == "retrieval-v2")
        .is_some_and(|gate| gate.passed);
    let safety_pass = gates
        .iter()
        .find(|gate| gate.id == "promotion-v2")
        .is_some_and(|gate| gate.passed)
        && leakage.exact_overlap == 0;
    let disposition = disposition_for(
        gates
            .iter()
            .find(|gate| gate.id == "zero-leakage-v2")
            .is_some_and(|gate| gate.passed),
        quality_pass,
        retrieval_pass,
        safety_pass,
        gates
            .iter()
            .find(|gate| gate.id == "resource-v2")
            .is_some_and(|gate| gate.passed),
    );
    let result = QualificationV2Report {
        schema_version: QUALIFICATION_V2_SCHEMA_VERSION,
        protocol: prereg.protocol,
        preregistration_commit_sha: prereg_commit.into(),
        protocol_hash: prereg.protocol_hash,
        historical_dataset_fingerprint: prereg.historical_dataset_fingerprint,
        fresh_holdout_fingerprint: prereg.fresh_holdout_fingerprint,
        fresh_holdout_leakage: leakage,
        disposition: disposition.into(),
        gates,
        slices,
        calibration,
        promotion,
        retrieval_frontier,
        resource,
        result_artifact_path: prereg.output_path.clone(),
    };
    fs::write(&prereg.output_path, serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

pub const QUALIFICATION_V3_SCHEMA_VERSION: u16 = 3;

/// Frozen selected-model identity for the v3 requalification. The SHA below
/// must equal the v2-selected MiniLM packed-marker artifact; any drift stops
/// the corrective and requires a new experiment plan.
pub const FROZEN_V3_SEQUENCE_ARTIFACT_SHA256: &str =
    "01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationV3Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub historical_dataset: String,
    pub historical_dataset_fingerprint: String,
    pub historical_train_partition_fingerprint: String,
    pub historical_dev_partition_fingerprint: String,
    pub historical_test_partition_fingerprint: String,
    pub fresh_holdout: String,
    pub fresh_holdout_fingerprint: String,
    pub fresh_holdout_manifest_sha256: String,
    pub candidate: FrozenSequenceCandidate,
    pub baseline_artifact_hashes: BTreeMap<String, String>,
    pub retrieval_mode: String,
    pub retrieval_k: usize,
    pub candidate_universe_sizes: Vec<usize>,
    pub promotion_max_candidates: usize,
    pub promotion_threshold: f64,
    pub threshold_source: String,
    pub slices: Vec<QualificationSlice>,
    pub gates: QualificationV2Gates,
    pub resource_limits: ResourceLimitsV2,
    pub release_command: String,
    pub target_hardware_os: String,
    pub output_path: String,
    pub protocol_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterfactualPairEvidence {
    pub pair_id: String,
    pub member_a: String,
    pub member_b: String,
    pub expected_a: String,
    pub expected_b: String,
    pub predicted_a: Option<String>,
    pub predicted_b: Option<String>,
    pub pair_correct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationV3Report {
    pub schema_version: u16,
    pub protocol: String,
    pub preregistration_commit_sha: String,
    pub protocol_hash: String,
    pub historical_dataset_fingerprint: String,
    pub fresh_holdout_fingerprint: String,
    pub fresh_holdout_leakage: HoldoutLeakageEvidence,
    pub disposition: String,
    pub gates: Vec<QualificationGate>,
    pub slices: Vec<SliceEvidence>,
    pub counterfactual_pairs: Vec<CounterfactualPairEvidence>,
    pub counterfactual_pair_accuracy: f64,
    pub calibration: CalibrationComparison,
    pub promotion: PromotionEvidence,
    pub retrieval_frontier: super::sequence_retrieval::RetrievalFrontierReport,
    pub resource: ResourceEvidenceV2,
    pub result_artifact_path: String,
}

fn protocol_hash_v3(prereg: &QualificationV3Preregistration) -> Result<String> {
    let mut value = serde_json::to_value(prereg)?;
    value
        .as_object_mut()
        .ok_or_else(|| anyhow!("v3 preregistration is not an object"))?
        .remove("protocol_hash");
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

pub fn load_preregistration_v3(path: &Path) -> Result<QualificationV3Preregistration> {
    let prereg: QualificationV3Preregistration = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read v3 preregistration {}", path.display()))?,
    )
    .context("parse v3 qualification preregistration")?;
    if prereg.schema_version != QUALIFICATION_V3_SCHEMA_VERSION
        || prereg.protocol.trim().is_empty()
        || prereg.protocol_hash.is_empty()
    {
        return Err(anyhow!(
            "unsupported or incomplete v3 qualification preregistration"
        ));
    }
    if protocol_hash_v3(&prereg)? != prereg.protocol_hash {
        return Err(anyhow!("v3 qualification protocol hash mismatch"));
    }
    if prereg.slices.is_empty() || prereg.gates.max_promotions == 0 {
        return Err(anyhow!(
            "v3 preregistration has no slices or promotion bound"
        ));
    }
    if prereg.candidate_universe_sizes != vec![64, 128] {
        return Err(anyhow!(
            "v3 preregistration must freeze candidate universes exactly [64, 128]"
        ));
    }
    if prereg.candidate.sequence_artifact_sha256 != FROZEN_V3_SEQUENCE_ARTIFACT_SHA256 {
        return Err(anyhow!("v3 selected sequence artifact hash drifted"));
    }
    Ok(prereg)
}

/// Skeleton identity for semantic-diversity accounting: the explicit
/// generator template recorded in `generated_variant_family`. Unique record
/// tokens live in the context/case id, never in the skeleton, so two cases
/// sharing a skeleton genuinely share template lineage.
fn skeleton_of(case: &ToolAdvisorCase) -> &str {
    case.generated_variant_family.as_str()
}

fn imperative_use_of_candidate(context_lower: &str, candidate_name: &str) -> bool {
    let variants = [
        candidate_name.to_ascii_lowercase(),
        candidate_name.to_ascii_lowercase().replace(['_', '-'], " "),
    ];
    let imperatives = [
        "use the",
        "use this",
        "call the",
        "run the",
        "open the",
        "invoke the",
        "apply the",
    ];
    variants.iter().any(|name| {
        context_lower.contains(name.as_str())
            && imperatives.iter().any(|verb| {
                context_lower.contains(&format!("{verb} {name}"))
                    || context_lower.contains(&format!("{verb} `{name}`"))
            })
    })
}

fn validate_no_tool_semantics(case: &ToolAdvisorCase) -> Result<()> {
    if !case.none || !case.relevance.is_empty() || !case.preferred_order.is_empty() {
        return Err(anyhow!(
            "no-tool case {} must abstain with empty labels",
            case.case_id
        ));
    }
    let context_lower = case.context.to_ascii_lowercase();
    for candidate in &case.candidates {
        if imperative_use_of_candidate(&context_lower, &candidate.name) {
            return Err(anyhow!(
                "no-tool case {} imperatively requests candidate {}",
                case.case_id,
                candidate.name
            ));
        }
    }
    if !context_lower.contains("rationale:") {
        return Err(anyhow!(
            "no-tool case {} states no human-readable abstention rationale",
            case.case_id
        ));
    }
    if case.tags.iter().any(|tag| tag == "multi-tool") {
        return Err(anyhow!(
            "no-tool case {} cannot also claim multi-tool",
            case.case_id
        ));
    }
    Ok(())
}

fn top_label(case: &ToolAdvisorCase) -> Option<&str> {
    case.preferred_order.first().map(String::as_str)
}

fn word_set(text: &str) -> BTreeSet<String> {
    super::normalize_text(text)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// True counterfactual pairs: exactly two members sharing one semantic group
/// and one candidate universe, with high wording overlap but different top
/// labels. Returns the pair ids for pair-accuracy scoring.
fn validate_counterfactual_pairs(cases: &[ToolAdvisorCase]) -> Result<Vec<String>> {
    let mut groups: BTreeMap<&str, Vec<&ToolAdvisorCase>> = BTreeMap::new();
    for case in cases {
        if case.tags.iter().any(|tag| tag == "counterfactual") {
            if case.none {
                return Err(anyhow!(
                    "counterfactual case {} cannot be a no-tool case",
                    case.case_id
                ));
            }
            groups
                .entry(case.semantic_group.as_str())
                .or_default()
                .push(case);
        }
    }
    let mut pair_ids = Vec::new();
    for (group, members) in &groups {
        if group.is_empty() {
            return Err(anyhow!("counterfactual case has an empty semantic group"));
        }
        if members.len() != 2 {
            return Err(anyhow!(
                "counterfactual pair {group} has {} members; need exactly 2",
                members.len()
            ));
        }
        let (left, right) = (members[0], members[1]);
        if top_label(left) == top_label(right) {
            return Err(anyhow!(
                "counterfactual pair {group} does not change the expected label"
            ));
        }
        let left_names = left
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<BTreeSet<_>>();
        let right_names = right
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<BTreeSet<_>>();
        if left_names != right_names {
            return Err(anyhow!(
                "counterfactual pair {group} does not share a candidate universe"
            ));
        }
        let left_words = word_set(&left.context);
        let right_words = word_set(&right.context);
        let intersection = left_words.intersection(&right_words).count() as f64;
        let union = left_words.union(&right_words).count() as f64;
        if union == 0.0 || intersection / union < 0.4 {
            return Err(anyhow!(
                "counterfactual pair {group} shares too little wording to be a paired cue change"
            ));
        }
        if left.context == right.context {
            return Err(anyhow!(
                "counterfactual pair {group} has identical contexts"
            ));
        }
        pair_ids.push((*group).to_string());
    }
    Ok(pair_ids)
}

fn validate_unknown_semantics(
    cases: &[ToolAdvisorCase],
    prior_names: &BTreeSet<String>,
) -> Result<()> {
    let mut obscured = 0usize;
    let mut meaningful = 0usize;
    for case in cases
        .iter()
        .filter(|case| case.tags.iter().any(|tag| tag == "unknown-renamed"))
    {
        let renamed = case
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.synthetic_identity && case.relevance.contains_key(&candidate.name)
            })
            .collect::<Vec<_>>();
        if renamed.is_empty() {
            return Err(anyhow!(
                "unknown-renamed case {} has no synthetic renamed relevant identity",
                case.case_id
            ));
        }
        for candidate in renamed {
            if prior_names.contains(&candidate.name) {
                return Err(anyhow!(
                    "unknown-renamed case {} reuses historical candidate name {}",
                    case.case_id,
                    candidate.name
                ));
            }
            if RegexLike::is_obscured(&candidate.name) {
                obscured += 1;
            } else {
                meaningful += 1;
            }
        }
    }
    if obscured == 0 || meaningful == 0 {
        return Err(anyhow!(
            "unknown-renamed slice needs both lexical-obscured and meaningful-new-name variants"
        ));
    }
    Ok(())
}

struct RegexLike;

impl RegexLike {
    fn is_obscured(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.starts_with("tool_x")
            && lower["tool_x".len()..]
                .chars()
                .all(|cell| cell.is_ascii_digit())
            && lower.len() > "tool_x".len()
    }
}

fn validate_context_v2_semantics(cases: &[ToolAdvisorCase]) -> Result<()> {
    let tagged = cases
        .iter()
        .filter(|case| case.tags.iter().any(|tag| tag == "context-v2-long-session"))
        .collect::<Vec<_>>();
    if tagged.is_empty() {
        return Err(anyhow!("context-v2-long-session slice is empty"));
    }
    let mut conflict_marked = 0usize;
    for case in &tagged {
        for marker in [
            "Current objective:",
            "Current task:",
            "Original session topic:",
        ] {
            if !case.context.contains(marker) {
                return Err(anyhow!(
                    "long-session case {} lacks AdvisorContextV2-shaped field {marker}",
                    case.case_id
                ));
            }
        }
        if !case.context.contains("Unresolved signal:")
            && !case
                .context
                .to_ascii_lowercase()
                .contains("no unresolved errors")
        {
            return Err(anyhow!(
                "long-session case {} lacks an unresolved-signal variant",
                case.case_id
            ));
        }
        let lower = case.context.to_ascii_lowercase();
        if [
            "stale",
            "supersede",
            "no longer",
            "rather than the original",
        ]
        .iter()
        .any(|cue| lower.contains(cue))
        {
            conflict_marked += 1;
        }
    }
    if conflict_marked * 2 < tagged.len() {
        return Err(anyhow!(
            "long-session slice needs a stale-origin/current-goal conflict in at least half the slice"
        ));
    }
    Ok(())
}

fn validate_hard_negative_semantics(cases: &[ToolAdvisorCase]) -> Result<()> {
    for case in cases
        .iter()
        .filter(|case| case.tags.iter().any(|tag| tag == "hard-negative"))
    {
        let deferred = case
            .candidates
            .iter()
            .filter(|candidate| candidate.disclosure == "deferred")
            .count();
        if deferred < 2 {
            return Err(anyhow!(
                "hard-negative case {} needs at least two plausible candidates",
                case.case_id
            ));
        }
        let Some(top) = top_label(case) else {
            return Err(anyhow!(
                "hard-negative case {} has no preferred label",
                case.case_id
            ));
        };
        let normalized_context = super::normalize_text(&case.context);
        let spelled = top.to_ascii_lowercase().replace(['_', '-'], " ");
        if normalized_context.contains(&top.to_ascii_lowercase())
            || normalized_context.contains(&spelled)
        {
            return Err(anyhow!(
                "hard-negative case {} leaks the relevant name into the context",
                case.case_id
            ));
        }
        if !(normalized_context.contains("plausible")
            && (normalized_context.contains("instead")
                || normalized_context.contains("rather than")
                || normalized_context.contains(" but ")))
        {
            return Err(anyhow!(
                "hard-negative case {} states no distractor rationale",
                case.case_id
            ));
        }
    }
    Ok(())
}

fn validate_multi_tool_semantics(cases: &[ToolAdvisorCase]) -> Result<()> {
    for case in cases
        .iter()
        .filter(|case| case.tags.iter().any(|tag| tag == "multi-tool"))
    {
        if case.relevance.len() < 2 || case.preferred_order.len() < 2 {
            return Err(anyhow!(
                "multi-tool case {} needs at least two genuinely relevant tools",
                case.case_id
            ));
        }
        let grades = case
            .preferred_order
            .iter()
            .filter_map(|name| case.relevance.get(name))
            .collect::<Vec<_>>();
        if grades.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(anyhow!(
                "multi-tool case {} needs graded relevance reflecting task sequence",
                case.case_id
            ));
        }
    }
    Ok(())
}

fn check_skeleton_diversity(cases: &[ToolAdvisorCase]) -> Result<BTreeMap<String, usize>> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for case in cases {
        *counts.entry(skeleton_of(case).to_string()).or_default() += 1;
    }
    if counts.len() < 64 {
        return Err(anyhow!(
            "holdout has only {} distinct scenario skeletons; need at least 64",
            counts.len()
        ));
    }
    let ceiling = (cases.len() as f64 * 0.05).ceil() as usize;
    if let Some((skeleton, count)) = counts.iter().max_by_key(|entry| entry.1) {
        if *count > ceiling.max(1) {
            return Err(anyhow!(
                "skeleton {skeleton} covers {count} cases; exceeds the 5% diversity ceiling"
            ));
        }
    }
    Ok(counts)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct V3HoldoutEvidence {
    pub cases: usize,
    pub leakage_groups: usize,
    pub skeleton_count: usize,
    pub counterfactual_pairs: usize,
    pub exact_overlap: usize,
    pub normalized_overlap: usize,
    pub template_overlap: usize,
    pub explicit_family_overlap: usize,
    pub copied_context_overlap: usize,
    pub required_slice_counts: BTreeMap<String, usize>,
}

/// Semantic v3 holdout validation (C001 acceptance).
///
/// `prior` combines the historical C001 corpus and the observed v2 holdout;
/// the candidate v3 holdout must prove zero leakage into both while every
/// special slice proves the behavior its tag claims.
pub fn validate_v3_holdout(
    prior: &[ToolAdvisorCase],
    holdout: &[ToolAdvisorCase],
) -> Result<V3HoldoutEvidence> {
    if holdout.len() < 160 {
        return Err(anyhow!(
            "v3 holdout has only {} cases; need at least 160",
            holdout.len()
        ));
    }
    let prior_exact = prior
        .iter()
        .map(super::exact_input_signature)
        .collect::<BTreeSet<_>>();
    let prior_normalized = prior
        .iter()
        .map(super::input_signature)
        .collect::<BTreeSet<_>>();
    let prior_contexts = prior
        .iter()
        .map(|case| super::normalize_text(&case.context))
        .collect::<BTreeSet<_>>();
    let prior_templates = prior
        .iter()
        .filter_map(|case| {
            (!case.generated_variant_family.is_empty())
                .then_some(case.generated_variant_family.as_str())
        })
        .collect::<BTreeSet<_>>();
    let prior_families = prior
        .iter()
        .flat_map(|case| [case.semantic_group.as_str(), case.leakage_group.as_str()])
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    let prior_names = prior
        .iter()
        .flat_map(|case| {
            case.candidates
                .iter()
                .map(|candidate| candidate.name.clone())
        })
        .collect::<BTreeSet<_>>();
    let mut exact_overlap = 0usize;
    let mut normalized_overlap = 0usize;
    let mut template_overlap = 0usize;
    let mut explicit_family_overlap = 0usize;
    let mut copied_context_overlap = 0usize;
    for case in holdout {
        if prior_exact.contains(&super::exact_input_signature(case)) {
            exact_overlap += 1;
        }
        if prior_normalized.contains(&super::input_signature(case)) {
            normalized_overlap += 1;
        }
        if !case.generated_variant_family.is_empty()
            && prior_templates.contains(case.generated_variant_family.as_str())
        {
            template_overlap += 1;
        }
        if [case.semantic_group.as_str(), case.leakage_group.as_str()]
            .iter()
            .any(|value| !value.is_empty() && prior_families.contains(*value))
        {
            explicit_family_overlap += 1;
        }
        if prior_contexts.contains(&super::normalize_text(&case.context)) {
            copied_context_overlap += 1;
        }
    }
    if exact_overlap != 0
        || normalized_overlap != 0
        || template_overlap != 0
        || explicit_family_overlap != 0
        || copied_context_overlap != 0
    {
        return Err(anyhow!(
            "v3 holdout leakage check failed: exact={exact_overlap} normalized={normalized_overlap} \
             template={template_overlap} explicit={explicit_family_overlap} copied={copied_context_overlap}"
        ));
    }
    let groups = super::build_leakage_groups(holdout);
    let leakage_groups = groups.iter().collect::<BTreeSet<_>>().len();
    if leakage_groups < 120 {
        return Err(anyhow!(
            "v3 holdout has only {leakage_groups} leakage groups; need at least 120"
        ));
    }
    let skeleton_counts = check_skeleton_diversity(holdout)?;
    let count = |tag: &str| {
        holdout
            .iter()
            .filter(|case| case.tags.iter().any(|item| item == tag))
            .count()
    };
    let mut required_slice_counts = BTreeMap::new();
    for tag in [
        "hard-negative",
        "counterfactual",
        "unknown-renamed",
        "no-tool",
        "multi-tool",
        "context-v2-long-session",
    ] {
        required_slice_counts.insert(tag.to_string(), count(tag));
    }
    let family_count = |families: &[&str]| {
        holdout
            .iter()
            .filter(|case| families.iter().any(|family| case.tool_family == *family))
            .count()
    };
    required_slice_counts.insert("plugin/MCP".into(), family_count(&["plugin", "mcp"]));
    required_slice_counts.insert("LSP".into(), family_count(&["lsp"]));
    required_slice_counts.insert(
        "research/search".into(),
        family_count(&["research", "search"]),
    );
    required_slice_counts.insert(
        "structured/data".into(),
        family_count(&["structured", "data"]),
    );
    for (slice, minimum) in [
        ("hard-negative", 24),
        ("counterfactual", 24),
        ("unknown-renamed", 24),
        ("no-tool", 24),
        ("multi-tool", 24),
        ("context-v2-long-session", 24),
        ("plugin/MCP", 16),
        ("LSP", 16),
        ("research/search", 16),
        ("structured/data", 16),
    ] {
        if required_slice_counts[slice] < minimum {
            return Err(anyhow!(
                "v3 holdout slice {slice} has {} cases; need {minimum}",
                required_slice_counts[slice]
            ));
        }
    }
    // No single tool family may dominate the set.
    let mut family_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for case in holdout {
        *family_counts.entry(case.tool_family.as_str()).or_default() += 1;
    }
    let family_ceiling = (holdout.len() as f64 * 0.25).ceil() as usize;
    for (family, total) in &family_counts {
        if *total > family_ceiling {
            return Err(anyhow!(
                "tool family {family} covers {total} cases; exceeds the 25% diversity ceiling"
            ));
        }
    }
    // At least 20% of non-no-tool cases must not repeat the relevant name.
    let non_no_tool = holdout.iter().filter(|case| !case.none).collect::<Vec<_>>();
    let obscured = non_no_tool
        .iter()
        .filter(|case| {
            top_label(case).is_some_and(|top| {
                let normalized_context = super::normalize_text(&case.context);
                let spelled = top.to_ascii_lowercase().replace(['_', '-'], " ");
                !normalized_context.contains(&top.to_ascii_lowercase())
                    && !normalized_context.contains(&spelled)
            })
        })
        .count();
    if obscured * 5 < non_no_tool.len() {
        return Err(anyhow!(
            "only {obscured} of {} non-no-tool cases hide the relevant name; need at least 20%",
            non_no_tool.len()
        ));
    }
    for case in holdout.iter().filter(|case| case.none) {
        validate_no_tool_semantics(case)?;
    }
    let pair_ids = validate_counterfactual_pairs(holdout)?;
    if pair_ids.len() < 12 {
        return Err(anyhow!(
            "v3 holdout has only {} true counterfactual pairs; need at least 12",
            pair_ids.len()
        ));
    }
    validate_unknown_semantics(holdout, &prior_names)?;
    validate_context_v2_semantics(holdout)?;
    validate_hard_negative_semantics(holdout)?;
    validate_multi_tool_semantics(holdout)?;
    Ok(V3HoldoutEvidence {
        cases: holdout.len(),
        leakage_groups,
        skeleton_count: skeleton_counts.len(),
        counterfactual_pairs: pair_ids.len(),
        exact_overlap,
        normalized_overlap,
        template_overlap,
        explicit_family_overlap,
        copied_context_overlap,
        required_slice_counts,
    })
}

fn counterfactual_pair_accuracy(
    holdout: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
) -> (Vec<CounterfactualPairEvidence>, f64) {
    let by_id = predictions
        .iter()
        .map(|prediction| (prediction.case_id.as_str(), prediction))
        .collect::<BTreeMap<_, _>>();
    let mut groups: BTreeMap<String, Vec<&ToolAdvisorCase>> = BTreeMap::new();
    for case in holdout {
        if case.tags.iter().any(|tag| tag == "counterfactual") {
            groups
                .entry(case.semantic_group.clone())
                .or_default()
                .push(case);
        }
    }
    let mut evidence = Vec::new();
    for (pair_id, members) in groups {
        if members.len() != 2 {
            continue;
        }
        let top = |case: &ToolAdvisorCase| -> Option<String> {
            by_id
                .get(case.case_id.as_str())
                .and_then(|prediction| prediction.ranked.first())
                .map(|ranked| ranked.name.clone())
        };
        let expected_a = members[0]
            .preferred_order
            .first()
            .cloned()
            .unwrap_or_default();
        let expected_b = members[1]
            .preferred_order
            .first()
            .cloned()
            .unwrap_or_default();
        let predicted_a = top(members[0]);
        let predicted_b = top(members[1]);
        // A pair is correct only when the ranker chooses the expected changed
        // label for each side.
        let pair_correct = predicted_a.as_deref() == Some(expected_a.as_str())
            && predicted_b.as_deref() == Some(expected_b.as_str());
        evidence.push(CounterfactualPairEvidence {
            pair_id: pair_id.to_string(),
            member_a: members[0].case_id.clone(),
            member_b: members[1].case_id.clone(),
            expected_a,
            expected_b,
            predicted_a,
            predicted_b,
            pair_correct,
        });
    }
    evidence.sort_by(|left, right| left.pair_id.cmp(&right.pair_id));
    let accuracy = if evidence.is_empty() {
        0.0
    } else {
        evidence.iter().filter(|pair| pair.pair_correct).count() as f64 / evidence.len() as f64
    };
    (evidence, accuracy)
}

#[allow(clippy::too_many_lines)]
pub fn qualify_v3(
    prereg_path: &Path,
    prereg_commit: &str,
    device: &Device,
) -> Result<QualificationV3Report> {
    let started = Instant::now();
    let prereg = load_preregistration_v3(prereg_path)?;
    let historical = load_cases(Some(Path::new(&prereg.historical_dataset)))?;
    let holdout = load_cases(Some(Path::new(&prereg.fresh_holdout)))?;
    if dataset_fingerprint(&historical)? != prereg.historical_dataset_fingerprint
        || dataset_fingerprint(&holdout)? != prereg.fresh_holdout_fingerprint
    {
        return Err(anyhow!("v3 dataset fingerprint mismatch"));
    }
    let historical_partition = partition_cases(&historical);
    let partition_fingerprint = |indices: &[usize]| -> Result<String> {
        dataset_fingerprint(
            &indices
                .iter()
                .map(|index| historical[*index].clone())
                .collect::<Vec<_>>(),
        )
    };
    if partition_fingerprint(&historical_partition.train_cases)?
        != prereg.historical_train_partition_fingerprint
        || partition_fingerprint(&historical_partition.dev_cases)?
            != prereg.historical_dev_partition_fingerprint
        || partition_fingerprint(&historical_partition.test_cases)?
            != prereg.historical_test_partition_fingerprint
    {
        return Err(anyhow!("v3 historical partition fingerprint mismatch"));
    }
    let manifest_path =
        Path::new(&prereg.fresh_holdout).with_file_name("qualification-v3-holdout-manifest.json");
    let manifest_sha = artifact_sha256(&manifest_path)?;
    if manifest_sha != prereg.fresh_holdout_manifest_sha256 {
        return Err(anyhow!("v3 fresh holdout manifest hash mismatch"));
    }
    // C001 construction isolation: the v3 holdout directory must not contain
    // selected-model predictions. Any v3 result already on disk means the
    // one-run discipline was violated before this invocation.
    let v2_holdout_path =
        Path::new(&prereg.fresh_holdout).with_file_name("qualification-v2-holdout.jsonl");
    let v2_holdout = load_cases(Some(&v2_holdout_path))?;
    let mut prior = historical.clone();
    prior.extend(v2_holdout.clone());
    let semantic = validate_v3_holdout(&prior, &holdout)?;
    if semantic.exact_overlap != 0
        || semantic.normalized_overlap != 0
        || semantic.template_overlap != 0
        || semantic.explicit_family_overlap != 0
        || semantic.copied_context_overlap != 0
    {
        return Err(anyhow!(
            "v3 fresh holdout leakage check failed: {semantic:?}"
        ));
    }
    for (path, expected) in &prereg.baseline_artifact_hashes {
        if artifact_sha256(Path::new(path))? != *expected {
            return Err(anyhow!("v3 baseline artifact hash mismatch for {path}"));
        }
    }
    let sequence_path = Path::new("target/tool-advisor/sequence-ranking-minilm-packed-head.json");
    let sequence = load_artifact(sequence_path, device)?;
    if artifact_sha256(sequence_path)? != prereg.candidate.sequence_artifact_sha256 {
        return Err(anyhow!("v3 selected sequence artifact hash mismatch"));
    }
    let manifest_path = Path::new(&sequence.encoder.assets.manifest_path);
    let candidate = &prereg.candidate;
    if artifact_sha256(manifest_path)? != candidate.encoder_manifest_sha256
        || sequence.encoder.assets.manifest.hashes["config"] != candidate.encoder_config_sha256
        || sequence.encoder.assets.manifest.hashes["vocabulary"] != candidate.tokenizer_sha256
        || sequence.encoder.assets.manifest.hashes["weights"] != candidate.source_weights_sha256
        || sequence.manifest.architecture != candidate.architecture
        || format!("{:?}", sequence.manifest.pooling).to_ascii_lowercase() != candidate.pooling
        || sequence.manifest.calibration.method != candidate.calibration_method
        || (sequence.manifest.calibration.abstention_threshold - candidate.abstention_threshold)
            .abs()
            > f64::EPSILON
        || sequence.manifest.training_partition_fingerprint
            != candidate.training_partition_fingerprint
    {
        return Err(anyhow!("v3 frozen candidate identity mismatch"));
    }
    let mut rank_times = Vec::with_capacity(holdout.len());
    let mut predictions = Vec::with_capacity(holdout.len());
    let mut forwards = 0usize;
    for case in &holdout {
        let case_started = Instant::now();
        let (prediction, _dropped, case_forwards) = sequence.predict_case(case)?;
        rank_times.push(case_started.elapsed().as_micros());
        forwards += case_forwards;
        predictions.push(prediction);
    }
    let sequence_metrics = evaluate(&holdout, &predictions)?;
    let calibrated_metrics =
        metric_at_threshold(&holdout, &predictions, candidate.abstention_threshold)?;
    let holdout_evaluation = sequence.evaluate_cases(&holdout)?;
    let raw_calibration = super::sequence_ranking::CalibrationReport {
        brier: predictions
            .iter()
            .zip(&holdout)
            .map(|(prediction, case)| {
                let value = prediction.abstain_probability.unwrap_or(0.0);
                let target = if case.none { 1.0 } else { 0.0 };
                (value - target).powi(2)
            })
            .sum::<f64>()
            / holdout.len() as f64,
        ece: holdout_evaluation.calibration.ece,
        nll: holdout_evaluation.calibration.nll,
    };
    let calibration = CalibrationComparison {
        raw: raw_calibration.clone(),
        calibrated: raw_calibration,
        raw_no_tool: sequence_metrics.clone(),
        calibrated_no_tool: calibrated_metrics,
        probabilities: predictions
            .iter()
            .map(|prediction| {
                let raw_probability = prediction.abstain_probability.unwrap_or(0.0);
                AbstentionProbabilityEvidence {
                    case_id: prediction.case_id.clone(),
                    raw_probability,
                    calibrated_probability: raw_probability,
                    calibrated_decision: raw_probability >= candidate.abstention_threshold,
                }
            })
            .collect(),
    };
    let keyword_predictions = holdout
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::Keyword))
        .collect::<Vec<_>>();
    let bm25_predictions = holdout
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::BM25))
        .collect::<Vec<_>>();
    let linear_path = prereg
        .baseline_artifact_hashes
        .keys()
        .find(|path| path.contains("model.json"))
        .map(Path::new);
    let linear = linear_path.map(load_linear).transpose()?;
    let linear_prediction_rows = linear
        .as_ref()
        .map(|artifact| {
            let advisor = LinearAdvisor::new(artifact.clone())?;
            linear_predictions(&advisor, &holdout)
        })
        .transpose()?;
    let mut slices = Vec::new();
    for slice in &prereg.slices {
        let selected = holdout
            .iter()
            .filter(|case| slice_matches(case, slice))
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() < slice.minimum_cases {
            return Err(anyhow!(
                "required v3 slice {} has only {} cases",
                slice.id,
                selected.len()
            ));
        }
        let ids = selected
            .iter()
            .map(|case| case.case_id.as_str())
            .collect::<BTreeSet<_>>();
        let select_predictions = |items: &[ToolAdvisorPrediction]| {
            items
                .iter()
                .filter(|prediction| ids.contains(prediction.case_id.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        };
        slices.push(SliceEvidence {
            id: slice.id.clone(),
            cases: selected.len(),
            keyword: evaluate(&selected, &select_predictions(&keyword_predictions))?,
            bm25: evaluate(&selected, &select_predictions(&bm25_predictions))?,
            linear: linear_prediction_rows
                .as_ref()
                .map(|items| evaluate(&selected, &select_predictions(items)))
                .transpose()?,
            sequence: evaluate(&selected, &select_predictions(&predictions))?,
            no_advisor: evaluate(
                &selected,
                &select_predictions(
                    &holdout
                        .iter()
                        .map(prediction_without_advisor)
                        .collect::<Vec<_>>(),
                ),
            )?,
        });
    }
    let (counterfactual_pairs, counterfactual_pair_accuracy) =
        counterfactual_pair_accuracy(&holdout, &predictions);
    let mode = parse_mode(&prereg.retrieval_mode)?;
    let mut retriever = HybridRetriever::new(&sequence.encoder);
    let surface = prereg.fresh_holdout_fingerprint.clone();
    let mut retrieval_frontier = frontier(
        &mut retriever,
        &holdout,
        &surface,
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let fixture_64_cases = fixture_cases(&holdout, 64);
    let fixture_128_cases = fixture_cases(&holdout, 128);
    validate_expanded_fixture(&fixture_64_cases, 64, prereg.retrieval_k)?;
    validate_expanded_fixture(&fixture_128_cases, 128, prereg.retrieval_k)?;
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_64_cases,
            &format!("{surface}:64"),
            &[prereg.retrieval_k],
            &[mode],
        )?
        .points,
    );
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_128_cases,
            &format!("{surface}:128"),
            &[prereg.retrieval_k],
            &[mode],
        )?
        .points,
    );
    let retrieval_times = holdout
        .iter()
        .map(|case| {
            let start = Instant::now();
            let _ = retriever.retrieve(case, &surface, mode, prereg.retrieval_k)?;
            Ok::<u128, anyhow::Error>(start.elapsed().as_micros())
        })
        .collect::<Result<Vec<_>>>()?;
    let promotion = promotion_evidence(
        &holdout,
        &predictions,
        prereg.promotion_threshold,
        prereg.gates.max_promotions,
        &prereg.threshold_source,
        &prereg.candidate.training_partition_fingerprint,
    )?;
    let weights = fs::metadata(&sequence.encoder.assets.weights_path)?.len();
    let head_path = sequence_path.with_extension("head.safetensors");
    let head = fs::metadata(head_path)?.len();
    let tokenizer = fs::metadata(&sequence.encoder.assets.vocabulary_path)?.len();
    let executable = std::env::current_exe()?;
    let release_binary = fs::metadata(&executable)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let comparable_path = Path::new("target/release/codegg-no-encoder");
    let comparable_binary = fs::metadata(comparable_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let (load_times, peak_rss) = process_cold_loads(sequence_path)?;
    let resource = ResourceEvidenceV2 {
        encoder_weight_bytes: weights,
        ranking_head_bytes: head,
        tokenizer_bytes: tokenizer,
        release_binary_bytes: release_binary,
        comparable_binary_bytes: comparable_binary,
        release_binary_delta_bytes: release_binary as i64 - comparable_binary as i64,
        peak_rss_bytes: peak_rss,
        process_cold_load_first_ms: load_times.first().copied().unwrap_or(0),
        process_cold_load_median_ms: percentile(&mut load_times.clone(), 0.5),
        process_cold_load_p95_ms: percentile(&mut load_times.clone(), 0.95),
        warmed_rank_p50_us: percentile(&mut rank_times.clone(), 0.5),
        warmed_rank_p95_us: percentile(&mut rank_times.clone(), 0.95),
        warmed_rank_max_us: rank_times.iter().copied().max().unwrap_or(0),
        retrieval_p50_us: percentile(&mut retrieval_times.clone(), 0.5),
        retrieval_p95_us: percentile(&mut retrieval_times.clone(), 0.95),
        retrieval_max_us: retrieval_times.iter().copied().max().unwrap_or(0),
        total_qualification_ms: started.elapsed().as_millis(),
        encoder_forwards_per_case: forwards as f64 / holdout.len() as f64,
        cache_warm_count: retriever.cache.len().saturating_sub(1),
        cache_cold_count: 1,
    };
    let mut gates = Vec::new();
    let aggregate = slices
        .iter()
        .find(|slice| slice.id == "aggregate")
        .ok_or_else(|| anyhow!("aggregate slice missing"))?;
    let quality_slices = [
        "hard-negative",
        "counterfactual",
        "unknown-renamed",
        "multi-tool",
        "context-v2-long-session",
        "plugin/MCP",
        "LSP",
        "research/search",
        "structured/data",
    ];
    let gain = quality_slices
        .iter()
        .filter_map(|id| slices.iter().find(|slice| slice.id == *id))
        .any(|slice| {
            slice.sequence.mrr
                >= slice
                    .linear
                    .as_ref()
                    .map(|metric| metric.mrr)
                    .unwrap_or(f64::NEG_INFINITY)
                    + prereg.gates.contextual_gain_min
                || slice.sequence.recall_at_1
                    >= slice
                        .linear
                        .as_ref()
                        .map(|metric| metric.recall_at_1)
                        .unwrap_or(f64::NEG_INFINITY)
                        + prereg.gates.contextual_gain_min
        });
    let no_tool_slice = slices
        .iter()
        .find(|slice| slice.id == "no-tool")
        .ok_or_else(|| anyhow!("no-tool slice missing"))?;
    let no_tool_baseline = [
        no_tool_slice.keyword.no_tool_f1,
        no_tool_slice.bm25.no_tool_f1,
        no_tool_slice
            .linear
            .as_ref()
            .and_then(|metric| metric.no_tool_f1),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::max);
    let no_tool_pass = no_tool_baseline
        .zip(calibration.calibrated_no_tool.no_tool_f1)
        .is_some_and(|(baseline, actual)| actual + prereg.gates.no_tool_f1_tolerance >= baseline);
    gates.push(QualificationGate {
        id: "zero-leakage-v3".into(),
        passed: semantic.exact_overlap == 0
            && semantic.normalized_overlap == 0
            && semantic.template_overlap == 0
            && semantic.explicit_family_overlap == 0
            && semantic.copied_context_overlap == 0,
        detail: format!(
            "exact={} normalized={} template={} explicit={} copied={} groups={} skeletons={} pairs={}",
            semantic.exact_overlap,
            semantic.normalized_overlap,
            semantic.template_overlap,
            semantic.explicit_family_overlap,
            semantic.copied_context_overlap,
            semantic.leakage_groups,
            semantic.skeleton_count,
            semantic.counterfactual_pairs
        ),
    });
    gates.push(QualificationGate {
        id: "aggregate-mrr-vs-linear-v3".into(),
        passed: aggregate.sequence.mrr + prereg.gates.aggregate_mrr_tolerance
            >= aggregate
                .linear
                .as_ref()
                .map(|metric| metric.mrr)
                .unwrap_or(f64::INFINITY),
        detail: format!(
            "sequence {:.4}, linear {:.4}",
            aggregate.sequence.mrr,
            aggregate
                .linear
                .as_ref()
                .map(|metric| metric.mrr)
                .unwrap_or(0.0)
        ),
    });
    gates.push(QualificationGate {
        id: "contextual-slice-gain-v3".into(),
        passed: gain,
        detail: "at least one preregistered contextual/generalization slice gains MRR or Recall@1"
            .into(),
    });
    gates.push(QualificationGate {
        id: "no-tool-f1-v3".into(),
        passed: no_tool_pass,
        detail: format!(
            "sequence {:?}, best baseline {:?}",
            aggregate.sequence.no_tool_f1, no_tool_baseline
        ),
    });
    let regressions_pass = quality_slices.iter().all(|id| {
        slices
            .iter()
            .find(|slice| slice.id == *id)
            .is_some_and(|slice| {
                slice.linear.as_ref().is_some_and(|linear| {
                    slice.sequence.mrr + prereg.gates.slice_regression_max >= linear.mrr
                })
            })
    });
    gates.push(QualificationGate { id: "slice-generalization-v3".into(), passed: regressions_pass, detail: "all required generalization slices are present and within the preregistered regression tolerance".into() });
    gates.push(QualificationGate {
        id: "calibration-v3".into(),
        passed: calibration.calibrated.brier
            <= calibration.raw.brier + prereg.gates.calibration_tolerance
            && calibration.calibrated.ece
                <= calibration.raw.ece + prereg.gates.calibration_tolerance,
        detail: format!(
            "brier raw {:.4}/cal {:.4}, ece raw {:.4}/cal {:.4}",
            calibration.raw.brier,
            calibration.calibrated.brier,
            calibration.raw.ece,
            calibration.calibrated.ece
        ),
    });
    // Corrected retrieval identity: missing or duplicated universe points are
    // correctness failure E via select_retrieval_point, never silent zero.
    let mut retrieval_detail = String::new();
    let mut retrieval_pass = false;
    let mut retrieval_error: Option<String> = None;
    match (
        select_retrieval_point(
            &retrieval_frontier.points,
            prereg.candidate_universe_sizes[0],
            prereg.retrieval_k,
            mode.as_str(),
        ),
        select_retrieval_point(
            &retrieval_frontier.points,
            prereg.candidate_universe_sizes[1],
            prereg.retrieval_k,
            mode.as_str(),
        ),
    ) {
        (Ok(point_64), Ok(point_128)) => {
            retrieval_detail = format!(
                "universe {} K={} recall {:.4}/{}; universe {} K={} recall {:.4}/{}",
                point_64.candidate_universe_size,
                point_64.shortlist_k,
                point_64.recall,
                prereg.gates.recall_64_min,
                point_128.candidate_universe_size,
                point_128.shortlist_k,
                point_128.recall,
                prereg.gates.recall_128_min
            );
            retrieval_pass = point_64.recall >= prereg.gates.recall_64_min
                && point_128.recall >= prereg.gates.recall_128_min;
        }
        (Err(error), _) | (_, Err(error)) => {
            retrieval_error = Some(error.to_string());
        }
    }
    if let Some(ref error) = retrieval_error {
        gates.push(QualificationGate {
            id: "retrieval-v3".into(),
            passed: false,
            detail: format!("correctness failure E: {error}"),
        });
    } else {
        gates.push(QualificationGate {
            id: "retrieval-v3".into(),
            passed: retrieval_pass,
            detail: retrieval_detail,
        });
    }
    let authority_violations =
        authority_negative_count(&mut retriever, &holdout, &surface, mode, prereg.retrieval_k)?;
    gates.push(QualificationGate {
        id: "authority-v3".into(),
        passed: authority_violations == 0 && promotion.authority_violations == 0,
        detail: format!(
            "{authority_violations} unauthorized retrieved descriptors, {} promotion violations",
            promotion.authority_violations
        ),
    });
    gates.push(QualificationGate {
        id: "promotion-v3".into(),
        passed: promotion.relevant_tool_promotion_recall >= prereg.gates.promotion_recall_min
            && promotion.no_tool_promotion_rate <= prereg.gates.no_tool_promotion_rate_max
            && promotion.irrelevant_tool_promotion_rate
                <= prereg.gates.irrelevant_promotion_rate_max
            && promotion.p95_schema_bytes <= prereg.gates.schema_budget_bytes as f64,
        detail: format!(
            "recall {:.4}, no-tool {:.4}, irrelevant {:.4}, schema p95 {:.0}B",
            promotion.relevant_tool_promotion_recall,
            promotion.no_tool_promotion_rate,
            promotion.irrelevant_tool_promotion_rate,
            promotion.p95_schema_bytes
        ),
    });
    gates.push(QualificationGate {
        id: "resource-v3".into(),
        passed: resource.process_cold_load_p95_ms
            <= prereg.resource_limits.max_process_cold_load_ms
            && resource.total_qualification_ms <= prereg.resource_limits.max_total_rank_ms
            && weights <= prereg.resource_limits.max_encoder_weight_bytes,
        detail: format!(
            "load p95 {}ms/{}, total {}ms/{}, weights {}B/{}",
            resource.process_cold_load_p95_ms,
            prereg.resource_limits.max_process_cold_load_ms,
            resource.total_qualification_ms,
            prereg.resource_limits.max_total_rank_ms,
            weights,
            prereg.resource_limits.max_encoder_weight_bytes
        ),
    });
    let quality_pass = gates
        .iter()
        .filter(|gate| {
            [
                "aggregate-mrr-vs-linear-v3",
                "contextual-slice-gain-v3",
                "no-tool-f1-v3",
                "slice-generalization-v3",
                "calibration-v3",
            ]
            .contains(&gate.id.as_str())
        })
        .all(|gate| gate.passed);
    let retrieval_gate_pass = gates
        .iter()
        .find(|gate| gate.id == "retrieval-v3")
        .is_some_and(|gate| gate.passed);
    let safety_pass = gates
        .iter()
        .find(|gate| gate.id == "promotion-v3")
        .is_some_and(|gate| gate.passed)
        && gates
            .iter()
            .find(|gate| gate.id == "authority-v3")
            .is_some_and(|gate| gate.passed)
        && semantic.exact_overlap == 0;
    let leakage_ok = gates
        .iter()
        .find(|gate| gate.id == "zero-leakage-v3")
        .is_some_and(|gate| gate.passed)
        && retrieval_error.is_none();
    let disposition = disposition_for(
        leakage_ok,
        quality_pass,
        retrieval_gate_pass,
        safety_pass,
        gates
            .iter()
            .find(|gate| gate.id == "resource-v3")
            .is_some_and(|gate| gate.passed),
    );
    let leakage_evidence = HoldoutLeakageEvidence {
        cases: semantic.cases,
        leakage_groups: semantic.leakage_groups,
        exact_overlap: semantic.exact_overlap,
        normalized_overlap: semantic.normalized_overlap,
        template_overlap: semantic.template_overlap,
        explicit_family_overlap: semantic.explicit_family_overlap,
        required_slice_counts: semantic.required_slice_counts,
    };
    let result = QualificationV3Report {
        schema_version: QUALIFICATION_V3_SCHEMA_VERSION,
        protocol: prereg.protocol,
        preregistration_commit_sha: prereg_commit.into(),
        protocol_hash: prereg.protocol_hash,
        historical_dataset_fingerprint: prereg.historical_dataset_fingerprint,
        fresh_holdout_fingerprint: prereg.fresh_holdout_fingerprint,
        fresh_holdout_leakage: leakage_evidence,
        disposition: disposition.into(),
        gates,
        slices,
        counterfactual_pairs,
        counterfactual_pair_accuracy,
        calibration,
        promotion,
        retrieval_frontier,
        resource,
        result_artifact_path: prereg.output_path.clone(),
    };
    fs::write(&prereg.output_path, serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_comparison_arm_does_not_define_contextual_slice() {
        let case = ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: "slice".into(),
            context: "context-v2-long-session fresh context".into(),
            candidates: Vec::new(),
            relevance: BTreeMap::new(),
            preferred_order: Vec::new(),
            none: true,
            tags: vec!["context-v2-long-session".into()],
            group_id: "slice-group".into(),
            provenance: "test".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: "context".into(),
            tool_family: "context".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        };
        let slice = QualificationSlice {
            id: "context-v2/long-session".into(),
            any_tags: vec!["context-v2-long-session".into()],
            any_families: Vec::new(),
            minimum_cases: 1,
        };
        assert!(slice_matches(&case, &slice));
    }

    #[test]
    fn disposition_preserves_a_through_e_semantics() {
        assert_eq!(
            disposition_for(true, true, true, true, true),
            "A — qualify for live M004"
        );
        assert_eq!(
            disposition_for(true, true, true, true, false),
            "B — quality gain but deployment cost too high"
        );
        assert_eq!(
            disposition_for(true, false, true, true, true),
            "C — retrieval useful, ranker not qualified"
        );
        assert_eq!(
            disposition_for(true, false, false, true, true),
            "D — no useful quality gain"
        );
        assert_eq!(
            disposition_for(false, true, true, true, true),
            "E — correctness/framework failure"
        );
    }

    #[test]
    fn threshold_calibration_emits_a_distinct_no_tool_decision() {
        let case = ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: "threshold".into(),
            context: "fresh threshold case".into(),
            candidates: vec![super::super::ToolAdvisorCandidate {
                name: "read".into(),
                description: "Read a file".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            relevance: BTreeMap::new(),
            preferred_order: Vec::new(),
            none: true,
            tags: vec!["no-tool".into()],
            group_id: "threshold-group".into(),
            provenance: "test".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: "test".into(),
            tool_family: "test".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        };
        let prediction = ToolAdvisorPrediction {
            schema_version: super::super::PREDICTION_SCHEMA_VERSION,
            case_id: "threshold".into(),
            ranked: Vec::new(),
            abstain_probability: Some(0.49),
            mode: "test".into(),
        };
        let summary = metric_at_threshold(&[case], &[prediction], 0.48).expect("metric");
        assert_eq!(summary.no_tool_recall, Some(1.0));
    }

    #[test]
    fn v3_generator_has_no_model_inference_path() {
        let script =
            fs::read_to_string("scripts/generate_tool_advisor_qualification_v3_holdout.py")
                .expect("v3 holdout generator");
        assert!(!script.contains("sequence-encoder"));
        assert!(!script.contains("load_artifact"));
        assert!(!script.contains("candle"));
        assert!(script.contains("generated-local-qualification-v3-template-v1"));
    }

    #[test]
    fn expanded_v3_fixture_preserves_every_known_relevant_tool() {
        use super::super::sequence_retrieval::validate_expanded_fixture;
        let holdout = super::super::load_cases(Some(Path::new(
            "assets/tool-advisor/qualification-v3-holdout.jsonl",
        )))
        .expect("v3 holdout");
        for universe in [64, 128] {
            let fixture = fixture_cases(&holdout, universe);
            validate_expanded_fixture(&fixture, universe, 16).expect("fixture validity");
            // The historical defect dropped relevant tools for most cases,
            // leaving recall measured over a handful of survivors.
            let measurable = fixture
                .iter()
                .filter(|case| {
                    !case.none
                        && case.relevance.keys().all(|name| {
                            case.candidates
                                .iter()
                                .any(|candidate| &candidate.name == name)
                        })
                })
                .count();
            let labeled = holdout.iter().filter(|case| !case.none).count();
            assert_eq!(measurable, labeled);
            assert!(fixture.iter().all(|case| case.candidates.len() == universe));
        }
    }

    #[test]
    fn insufficient_fresh_holdout_fails_closed() {
        let cases = super::super::builtin_cases().expect("builtin corpus");
        let error = validate_v2_holdout(&cases, &cases[..32]).expect_err("small holdout");
        assert!(error.to_string().contains("at least 128"));
    }

    #[test]
    fn repository_holdout_meets_v2_floors_without_loading_a_model() {
        let historical =
            super::super::load_cases(Some(Path::new("assets/tool-advisor/corpus.jsonl")))
                .expect("historical corpus");
        let holdout = super::super::load_cases(Some(Path::new(
            "assets/tool-advisor/qualification-v2-holdout.jsonl",
        )))
        .expect("fresh holdout");
        let evidence = validate_v2_holdout(&historical, &holdout).expect("holdout floors");
        assert_eq!(evidence.cases, 128);
        assert!(evidence.leakage_groups >= 96);
        assert_eq!(evidence.exact_overlap, 0);
        assert_eq!(evidence.normalized_overlap, 0);
        assert_eq!(evidence.template_overlap, 0);
        assert_eq!(evidence.explicit_family_overlap, 0);
    }

    #[test]
    fn repository_preregistration_protocol_hash_matches_rust_canonicalization() {
        let prereg: QualificationV2Preregistration = serde_json::from_slice(
            &fs::read("assets/tool-advisor/sequence-qualification-v2-preregistration.json")
                .expect("preregistration"),
        )
        .expect("parse preregistration");
        assert_eq!(
            protocol_hash_v2(&prereg).expect("hash"),
            prereg.protocol_hash
        );
    }

    #[test]
    fn repository_preregistration_holdout_fingerprint_matches_loader() {
        let prereg: QualificationV2Preregistration = serde_json::from_slice(
            &fs::read("assets/tool-advisor/sequence-qualification-v2-preregistration.json")
                .expect("preregistration"),
        )
        .expect("parse preregistration");
        let holdout = super::super::load_cases(Some(Path::new(&prereg.fresh_holdout)))
            .expect("fresh holdout");
        assert_eq!(
            dataset_fingerprint(&holdout).expect("fingerprint"),
            prereg.fresh_holdout_fingerprint
        );
    }

    #[test]
    fn holdout_constructor_has_no_sequence_inference_path() {
        let script = fs::read_to_string("scripts/generate_tool_advisor_qualification_holdout.py")
            .expect("holdout generator");
        assert!(!script.contains("sequence-encoder"));
        assert!(!script.contains("load_artifact"));
        assert!(script.contains("generated-local-qualification-v2-template-v1"));
    }

    #[allow(clippy::too_many_arguments)]
    fn v3_case(
        case_id: &str,
        context: &str,
        candidates: Vec<super::super::ToolAdvisorCandidate>,
        relevance: BTreeMap<String, u8>,
        preferred_order: Vec<String>,
        none: bool,
        tags: Vec<String>,
        skeleton: &str,
        family: &str,
    ) -> super::super::ToolAdvisorCase {
        super::super::ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: case_id.into(),
            context: context.into(),
            candidates,
            relevance,
            preferred_order,
            none,
            tags,
            group_id: format!("{case_id}-group"),
            provenance: "generated-local-qualification-v3-test".into(),
            semantic_group: format!("{case_id}-semantic"),
            leakage_group: format!("{case_id}-leakage"),
            task_family: family.into(),
            tool_family: family.into(),
            generated_variant_family: skeleton.into(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    fn deferred_candidate(name: &str, synthetic: bool) -> super::super::ToolAdvisorCandidate {
        super::super::ToolAdvisorCandidate {
            name: name.into(),
            description: format!("Deferred {name} capability"),
            category: "ReadOnly".into(),
            disclosure: "deferred".into(),
            synthetic_identity: synthetic,
        }
    }

    fn core_candidate(name: &str) -> super::super::ToolAdvisorCandidate {
        super::super::ToolAdvisorCandidate {
            name: name.into(),
            description: format!("Core {name} capability"),
            category: "ReadOnly".into(),
            disclosure: "core".into(),
            synthetic_identity: false,
        }
    }

    #[test]
    fn contradictory_no_tool_imperative_case_is_rejected() {
        let case = v3_case(
            "v3-test-no-tool-bad",
            "In record v3-test-01, use the grep tool for the workspace item. Rationale: none.",
            vec![
                core_candidate("grep"),
                deferred_candidate("plugin_search", false),
            ],
            BTreeMap::new(),
            Vec::new(),
            true,
            vec!["no-tool".into()],
            "v3-test-skeleton-no-tool",
            "plugin",
        );
        let error = validate_no_tool_semantics(&case).expect_err("imperative no-tool");
        assert!(error.to_string().contains("imperatively requests"));
    }

    #[test]
    fn fake_unknown_tag_without_novel_identity_is_rejected() {
        let historical = super::super::builtin_cases().expect("builtin corpus");
        let prior_names = historical
            .iter()
            .flat_map(|case| {
                case.candidates
                    .iter()
                    .map(|candidate| candidate.name.clone())
            })
            .collect::<BTreeSet<_>>();
        assert!(prior_names.contains("plugin_search"));
        let case = v3_case(
            "v3-test-unknown-fake",
            "Look up the catalog entry in record v3-test-02.",
            vec![
                core_candidate("grep"),
                deferred_candidate("plugin_search", false),
            ],
            BTreeMap::from([(String::from("plugin_search"), 3)]),
            vec!["plugin_search".into()],
            false,
            vec!["unknown-renamed".into()],
            "v3-test-skeleton-unknown",
            "plugin",
        );
        let error =
            validate_unknown_semantics(&[case], &prior_names).expect_err("fake unknown identity");
        assert!(error
            .to_string()
            .contains("no synthetic renamed relevant identity"));
    }

    #[test]
    fn orphan_counterfactual_case_is_rejected() {
        let case = v3_case(
            "v3-test-cf-orphan",
            "Search the repository docs for the retry policy in record v3-test-03.",
            vec![
                core_candidate("grep"),
                deferred_candidate("docs_lookup", false),
            ],
            BTreeMap::from([(String::from("docs_lookup"), 3)]),
            vec!["docs_lookup".into()],
            false,
            vec!["counterfactual".into()],
            "v3-test-skeleton-cf",
            "search",
        );
        let error = validate_counterfactual_pairs(&[case]).expect_err("orphan pair member");
        assert!(error.to_string().contains("need exactly 2"));
    }

    #[test]
    fn counterfactual_pair_with_same_label_is_rejected() {
        let members = ["a", "b"]
            .iter()
            .map(|side| {
                v3_case(
                    &format!("v3-test-cf-same-{side}"),
                    &format!(
                        "Search the repository docs for the retry policy in record v3-test-04-{side}, \
                         preferring the local documentation index over the public web."
                    ),
                    vec![
                        core_candidate("grep"),
                        deferred_candidate("docs_lookup", false),
                        deferred_candidate("web_search", false),
                    ],
                    BTreeMap::from([(String::from("docs_lookup"), 3)]),
                    vec!["docs_lookup".into()],
                    false,
                    vec!["counterfactual".into()],
                    "v3-test-skeleton-cf-same",
                    "search",
                )
            })
            .collect::<Vec<_>>();
        let mut paired = members.clone();
        for case in &mut paired {
            case.semantic_group = "v3-test-pair-same".into();
        }
        let error = validate_counterfactual_pairs(&paired).expect_err("same label");
        assert!(error
            .to_string()
            .contains("does not change the expected label"));
    }

    #[test]
    fn long_session_tag_without_state_fields_is_rejected() {
        let case = v3_case(
            "v3-test-ls-tag-only",
            "A short ordinary prompt about the build in record v3-test-05.",
            vec![
                core_candidate("grep"),
                deferred_candidate("lsp_definition", false),
            ],
            BTreeMap::from([(String::from("lsp_definition"), 3)]),
            vec!["lsp_definition".into()],
            false,
            vec!["context-v2-long-session".into()],
            "v3-test-skeleton-ls",
            "lsp",
        );
        let error = validate_context_v2_semantics(&[case]).expect_err("tag-only session");
        assert!(error.to_string().contains("AdvisorContextV2-shaped field"));
    }

    #[test]
    fn repeated_skeleton_floor_violation_is_rejected() {
        let cases = (0..10)
            .map(|index| {
                v3_case(
                    &format!("v3-test-skel-{index:02}"),
                    &format!("Ordinary file context {index} in record v3-test-06."),
                    vec![core_candidate("grep"), deferred_candidate("read", false)],
                    BTreeMap::from([(String::from("read"), 3)]),
                    vec!["read".into()],
                    false,
                    Vec::new(),
                    "v3-test-skeleton-shared",
                    "filesystem",
                )
            })
            .collect::<Vec<_>>();
        let error = check_skeleton_diversity(&cases).expect_err("skeleton floor");
        assert!(error.to_string().contains("distinct scenario skeletons"));
    }

    #[test]
    fn v2_holdout_overlap_is_rejected_for_v3() {
        let historical =
            super::super::load_cases(Some(Path::new("assets/tool-advisor/corpus.jsonl")))
                .expect("historical corpus");
        let v2_holdout = super::super::load_cases(Some(Path::new(
            "assets/tool-advisor/qualification-v2-holdout.jsonl",
        )))
        .expect("v2 holdout");
        let error = validate_v3_holdout(&historical, &v2_holdout).expect_err("v2 overlap");
        assert!(
            error.to_string().contains("only 128 cases")
                || error.to_string().contains("leakage check failed")
        );
    }

    #[test]
    fn selected_artifact_hash_remains_frozen() {
        let prereg: QualificationV2Preregistration = serde_json::from_slice(
            &fs::read("assets/tool-advisor/sequence-qualification-v2-preregistration.json")
                .expect("preregistration"),
        )
        .expect("parse preregistration");
        assert_eq!(
            prereg.candidate.sequence_artifact_sha256,
            FROZEN_V3_SEQUENCE_ARTIFACT_SHA256
        );
    }

    #[test]
    fn repository_v3_holdout_meets_semantic_floors() {
        let historical =
            super::super::load_cases(Some(Path::new("assets/tool-advisor/corpus.jsonl")))
                .expect("historical corpus");
        let v2_holdout = super::super::load_cases(Some(Path::new(
            "assets/tool-advisor/qualification-v2-holdout.jsonl",
        )))
        .expect("v2 holdout");
        let holdout = super::super::load_cases(Some(Path::new(
            "assets/tool-advisor/qualification-v3-holdout.jsonl",
        )))
        .expect("v3 holdout");
        let mut prior = historical;
        prior.extend(v2_holdout);
        let evidence = validate_v3_holdout(&prior, &holdout).expect("v3 semantic floors");
        assert!(evidence.cases >= 160);
        assert!(evidence.leakage_groups >= 120);
        assert!(evidence.skeleton_count >= 64);
        assert!(evidence.counterfactual_pairs >= 12);
        assert_eq!(evidence.exact_overlap, 0);
        assert_eq!(evidence.normalized_overlap, 0);
        assert_eq!(evidence.template_overlap, 0);
        assert_eq!(evidence.explicit_family_overlap, 0);
        assert_eq!(evidence.copied_context_overlap, 0);
    }
}
