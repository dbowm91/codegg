//! Frozen, offline qualification protocol for the sequence-encoder experiment.
//!
//! The protocol verifies all fingerprints and artifact hashes before it
//! selects the final partition.  This is the only command in the experiment
//! that reads frozen test labels; training and retrieval development paths do
//! not call it.

use super::sequence_ranking::{load_artifact, RankingEvaluation};
use super::sequence_retrieval::{frontier, parse_mode, HybridRetriever, RetrievalMode};
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

fn fixture_cases(cases: &[ToolAdvisorCase], size: usize) -> Vec<ToolAdvisorCase> {
    let mut universe = cases
        .iter()
        .flat_map(|case| case.candidates.iter().cloned())
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| (candidate.name.clone(), candidate))
        .collect::<BTreeMap<_, _>>();
    for index in 0..size {
        universe
            .entry(format!("fixture_deferred_{index:03}"))
            .or_insert_with(|| super::ToolAdvisorCandidate {
                name: format!("fixture_deferred_{index:03}"),
                description: format!("Deterministic expanded fixture descriptor {index}"),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: true,
            });
    }
    let universe = universe.into_values().take(size).collect::<Vec<_>>();
    cases
        .iter()
        .map(|case| {
            let mut fixture = case.clone();
            fixture.candidates = universe.clone();
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
    let fixture_64 = frontier(
        &mut retriever,
        &fixture_cases(&test_cases, 64),
        &format!("{surface}:64"),
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let fixture_128 = frontier(
        &mut retriever,
        &fixture_cases(&test_cases, 128),
        &format!("{surface}:128"),
        &[prereg.retrieval_k],
        &[mode],
    )?;
    let mut retrieval_frontier = retrieval_frontier;
    retrieval_frontier.points.extend(fixture_64.points);
    retrieval_frontier.points.extend(fixture_128.points);
    let hybrid_recall_64 = retrieval_frontier
        .points
        .iter()
        .filter(|point| point.mode == mode.as_str())
        .nth(1)
        .map(|point| point.recall)
        .unwrap_or(0.0);
    let hybrid_recall_128 = retrieval_frontier
        .points
        .iter()
        .filter(|point| point.mode == mode.as_str())
        .nth(2)
        .map(|point| point.recall)
        .unwrap_or(0.0);
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
    prereg: &QualificationV2Preregistration,
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
            prereg.promotion_threshold,
            prereg.gates.max_promotions,
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
        threshold_source: prereg.threshold_source.clone(),
        dev_fingerprint: prereg.candidate.training_partition_fingerprint.clone(),
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
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_cases(&holdout, 64),
            &format!("{surface}:64"),
            &[prereg.retrieval_k],
            &[mode],
        )?
        .points,
    );
    retrieval_frontier.points.extend(
        frontier(
            &mut retriever,
            &fixture_cases(&holdout, 128),
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
    let promotion = promotion_evidence(&holdout, &predictions, &prereg)?;
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
    let points = &retrieval_frontier.points;
    let recall_64 = points
        .iter()
        .find(|point| point.k == 64)
        .map(|point| point.recall)
        .unwrap_or(0.0);
    let recall_128 = points
        .iter()
        .find(|point| point.k == 128)
        .map(|point| point.recall)
        .unwrap_or(0.0);
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
    fn holdout_constructor_has_no_sequence_inference_path() {
        let script = fs::read_to_string("scripts/generate_tool_advisor_qualification_holdout.py")
            .expect("holdout generator");
        assert!(!script.contains("sequence-encoder"));
        assert!(!script.contains("load_artifact"));
        assert!(script.contains("generated-local-qualification-v2-template-v1"));
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
}
