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
use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

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
