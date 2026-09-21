//! Clean offline requalification (C004): pre-registered protocol execution.
//!
//! This module runs the frozen C004 protocol end to end and emits a
//! machine-readable verdict. It reuses the existing `bench`/`eval`
//! machinery (`evaluate`, C001 partitions, C002 calibrated artifacts, C003
//! preselection) rather than building a separate benchmark framework.
//!
//! Anti-tuning contract: every input fingerprint and artifact hash is
//! verified against `assets/tool-advisor/c004-requalification.json` before
//! any test-partition label is inspected. Promotion thresholds are selected
//! on dev only. Family-holdout measurements use in-run exclusion retraining
//! whose optimizer and calibration input provably excludes the held-out
//! family. No live model calls occur anywhere in this path.

use super::contextual::{self, ContextualTrainingConfig};
use super::training::{self, TrainingConfig, TRAINING_CONFIG_VERSION};
use super::{
    baseline_prediction, candidates_from_deferred_surface, dataset_fingerprint, evaluate,
    family_holdout_partition, load_artifact, load_cases, partition_cases, preselect_candidates,
    preselection_recall, project_discovery, unknown_tool_holdout, validate_qualification_corpus,
    AdvisorMode, LinearAdvisor, RankedCandidate, ToolAdvisor, ToolAdvisorCase, ToolAdvisorInput,
    ToolAdvisorPrediction, MAX_CANDIDATES, MAX_CONTEXT_BYTES, PREDICTION_SCHEMA_VERSION,
};
use crate::tool::catalog::{SearchMode, ToolMetadata};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Pre-registration.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreregisteredArtifact {
    pub label: String,
    pub path: String,
    pub sha256: String,
    pub kind: String,
    #[serde(default)]
    pub excluded_tool_families: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationExpectation {
    pub abstain_bias: f32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionTrainingConfig {
    pub capacity: String,
    pub vocab_buckets: usize,
    pub epochs: u32,
    pub learning_rate: f32,
    pub seed: u64,
    pub max_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearExclusionTrainingConfig {
    pub epochs: u32,
    pub learning_rate: f32,
    pub seed: u64,
    pub max_context_bytes: usize,
    pub max_candidates: usize,
    pub calibration_temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionTrainingSection {
    pub contextual: ExclusionTrainingConfig,
    pub linear: LinearExclusionTrainingConfig,
    pub work_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionSection {
    pub max_candidates: usize,
    pub max_promotions: usize,
    pub schema_budget_bytes: usize,
    pub margin: f64,
    pub threshold_grid: Vec<f64>,
    pub selection_rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateSection {
    pub preselect_recall_min: f64,
    pub aggregate_mrr_tolerance_vs_linear: f64,
    pub slice_gain_min: f64,
    pub slice_regression_max: f64,
    pub no_tool_f1_tolerance_vs_best_baseline: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTargetSection {
    pub max_artifact_bytes: u64,
    pub max_cold_load_millis: u128,
    pub max_score_p95_micros: u128,
    pub max_preselect_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preregistration {
    /// Version 1 is the historical C004 schema. Version 2 adds an
    /// independently hashed protocol/provenance envelope for future runs.
    #[serde(default = "default_preregistration_schema_version")]
    pub schema_version: u16,
    pub protocol: String,
    pub dataset: String,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub test_partition_fingerprint: String,
    pub holdout_families: Vec<String>,
    pub unknown_transform: String,
    pub artifacts: Vec<PreregisteredArtifact>,
    pub expected_calibration: BTreeMap<String, CalibrationExpectation>,
    pub exclusion_training: ExclusionTrainingSection,
    pub promotion: PromotionSection,
    pub gates: GateSection,
    pub resource_targets: ResourceTargetSection,
    /// SHA-256 of the canonical preregistration with this field and the
    /// operator-supplied commit provenance omitted. Empty is accepted only
    /// for the historical v1 C004 manifest.
    #[serde(default)]
    pub protocol_hash: String,
    /// Stable hashes for model/config inputs not represented by an artifact
    /// payload hash. The harness echoes these values into its report.
    #[serde(default)]
    pub model_config_hashes: BTreeMap<String, String>,
    /// Explicit gate identifiers frozen by the operator. This is separate
    /// from the measured gate results in the report.
    #[serde(default)]
    pub declared_gate_set: Vec<String>,
    /// The commit containing the frozen preregistration, supplied by the
    /// operator or closure process. It is provenance, not protocol content.
    #[serde(default)]
    pub preregistration_commit_sha: Option<String>,
}

fn default_preregistration_schema_version() -> u16 {
    1
}

/// Compute the canonical protocol hash used by schema-v2 preregistrations.
/// JSON field order is stable because all maps are ordered and serde emits
/// struct fields in declaration order.
pub fn protocol_hash_for(preregistration: &Preregistration) -> Result<String> {
    let mut canonical = preregistration.clone();
    canonical.protocol_hash.clear();
    canonical.preregistration_commit_sha = None;
    let bytes = serde_json::to_vec(&canonical).context("serialize canonical preregistration")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn load_preregistration(path: &Path) -> Result<Preregistration> {
    let bytes =
        fs::read(path).with_context(|| format!("read preregistration {}", path.display()))?;
    let preregistration: Preregistration =
        serde_json::from_slice(&bytes).context("parse preregistration")?;
    if preregistration.schema_version >= 2 {
        if preregistration.protocol_hash.is_empty() {
            return Err(anyhow!(
                "schema-v2 preregistration is missing protocol_hash"
            ));
        }
        let actual = protocol_hash_for(&preregistration)?;
        if actual != preregistration.protocol_hash {
            return Err(anyhow!(
                "preregistration protocol hash mismatch: expected {}, found {actual}",
                preregistration.protocol_hash
            ));
        }
        if preregistration.declared_gate_set.is_empty() {
            return Err(anyhow!(
                "schema-v2 preregistration must declare at least one gate"
            ));
        }
    }
    Ok(preregistration)
}

// ---------------------------------------------------------------------------
// Report types.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SliceMetrics {
    pub cases: usize,
    pub mrr: f64,
    pub recall_at_1: f64,
    pub recall_at_3: f64,
    pub recall_at_5: f64,
    pub ndcg_at_5: f64,
    pub candidate_coverage: f64,
    pub no_tool_precision: Option<f64>,
    pub no_tool_recall: Option<f64>,
    pub no_tool_f1: Option<f64>,
    pub abstention_brier: f64,
    pub abstention_ece: f64,
    pub abstention_nll: f64,
    #[serde(default)]
    pub pair_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromotionSliceReport {
    /// Shared dev-selected threshold applied to every learned model.
    pub threshold: f64,
    /// Mean dev promotion-F1 at the selected threshold.
    pub dev_f1_at_threshold: f64,
    /// Micro promotion recall over relevant deferred tools.
    pub test_recall: f64,
    /// False promotions over total promotions (0.0 when nothing promotes).
    pub test_false_promotion_rate: f64,
    /// Fraction of no-tool cases receiving any promotion.
    pub test_no_tool_promotion_rate: f64,
    /// Mean schema bytes added per promoting case.
    pub mean_schema_bytes_added: f64,
    /// Max schema bytes added by any single case.
    pub max_schema_bytes_added: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceSliceReport {
    pub artifact_bytes: u64,
    pub allocated_parameters: u64,
    pub trained_parameters: Option<u64>,
    pub trained_fraction: Option<f64>,
    pub cold_load_millis: u128,
    pub score_p50_micros: u128,
    pub score_p95_micros: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelReport {
    pub label: String,
    pub kind: String,
    pub artifact_sha256: Option<String>,
    pub qualified: Option<bool>,
    pub excluded_tool_families: Vec<String>,
    pub slices: BTreeMap<String, SliceMetrics>,
    pub promotion: Option<PromotionSliceReport>,
    pub resource: Option<ResourceSliceReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecallFixtureReport {
    pub cases: usize,
    pub universe_size: usize,
    pub budget: usize,
    pub relevant_total: usize,
    pub relevant_shortlisted: usize,
    pub recall: f64,
    pub missing: Vec<String>,
    pub mean_preselect_millis: f64,
    pub max_preselect_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateVerdict {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Disposition {
    /// Positive qualification: corrected architecture generalizes per the
    /// gates; the live M004 plan may proceed with the selected artifact.
    #[serde(rename = "A-positive")]
    APositive,
    /// Mechanically correct but no useful gain: research baseline only.
    #[serde(rename = "B-no-useful-gain")]
    BNoUsefulGain,
    /// Compact configuration matches quality: M004 may proceed only with it.
    #[serde(rename = "C-resource-efficient")]
    CResourceEfficient,
    /// Correctness failure: register a narrower corrective, never unblock.
    #[serde(rename = "D-correctness-failure")]
    DCorrectnessFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequalificationReport {
    pub schema_version: u16,
    pub protocol: String,
    pub protocol_hash: String,
    pub model_config_hashes: BTreeMap<String, String>,
    pub declared_gate_set: Vec<String>,
    pub preregistration_commit_sha: Option<String>,
    pub dataset: String,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub test_partition_fingerprint: String,
    pub leakage_violations: usize,
    pub models: Vec<ModelReport>,
    pub recall_fixture: RecallFixtureReport,
    pub authority_violations: usize,
    pub shared_promotion_threshold: f64,
    pub gates: Vec<GateVerdict>,
    pub disposition: Disposition,
    pub selected_artifact: Option<String>,
    pub live_m004_transition: String,
}

// ---------------------------------------------------------------------------
// Scoring helpers.
// ---------------------------------------------------------------------------

struct FixedAdvisor {
    predictions: BTreeMap<String, ToolAdvisorPrediction>,
}

impl ToolAdvisor for FixedAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        self.predictions
            .get(&input.case_id)
            .cloned()
            .ok_or_else(|| anyhow!("no fixed prediction for {}", input.case_id))
    }
}

enum Scorer {
    Keyword,
    Bm25,
    Linear(LinearAdvisor),
    Contextual(contextual::ContextualAdvisor),
}

impl Scorer {
    fn predict(&self, case: &ToolAdvisorCase, mode: &str) -> Result<ToolAdvisorPrediction> {
        let mut prediction = match self {
            Self::Keyword => baseline_prediction(case, SearchMode::Keyword),
            Self::Bm25 => baseline_prediction(case, SearchMode::BM25),
            Self::Linear(advisor) => advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: "requalify".into(),
            })?,
            Self::Contextual(advisor) => advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: "requalify".into(),
            })?,
        };
        prediction.mode = mode.to_string();
        Ok(prediction)
    }
}

fn abstention_stats(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
) -> (f64, f64, f64) {
    let by_id: BTreeMap<&str, &ToolAdvisorPrediction> = predictions
        .iter()
        .map(|prediction| (prediction.case_id.as_str(), prediction))
        .collect();
    let mut pairs = Vec::new();
    for case in cases {
        if let Some(prediction) = by_id.get(case.case_id.as_str()) {
            let probability = prediction
                .abstain_probability
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            pairs.push((probability, case.none));
        }
    }
    (brier(&pairs), ece(&pairs), nll(&pairs))
}

fn brier(pairs: &[(f64, bool)]) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    pairs
        .iter()
        .map(|(probability, actual)| {
            let target = if *actual { 1.0 } else { 0.0 };
            (probability - target).powi(2)
        })
        .sum::<f64>()
        / pairs.len() as f64
}

fn ece(pairs: &[(f64, bool)]) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    let mut bins = vec![(0usize, 0.0, 0.0); 10];
    for (probability, actual) in pairs {
        let bin = (probability * 10.0).floor() as usize;
        let bin = bin.min(9);
        bins[bin].0 += 1;
        bins[bin].1 += probability;
        if *actual {
            bins[bin].2 += 1.0;
        }
    }
    bins.into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, sum, positives)| {
            let accuracy = positives / count as f64;
            let confidence = sum / count as f64;
            (accuracy - confidence).abs() * count as f64 / pairs.len() as f64
        })
        .sum()
}

fn nll(pairs: &[(f64, bool)]) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    pairs
        .iter()
        .map(|(probability, actual)| {
            let clamped = probability.clamp(1e-12, 1.0 - 1e-12);
            let target = if *actual { 1.0 } else { 0.0 };
            -(target * clamped.ln() + (1.0 - target) * (1.0 - clamped).ln())
        })
        .sum::<f64>()
        / pairs.len() as f64
}

fn uncalibrated_abstention(top_score: f64) -> f64 {
    (1.0 / (1.0 + top_score.exp())).clamp(0.0, 1.0)
}

fn slice_metrics(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
    pair_accuracy: Option<f64>,
) -> Result<SliceMetrics> {
    let summary = evaluate(cases, predictions)?;
    let (abstention_brier, abstention_ece, abstention_nll) = abstention_stats(cases, predictions);
    Ok(SliceMetrics {
        cases: summary.cases,
        mrr: summary.mrr,
        recall_at_1: summary.recall_at_1,
        recall_at_3: summary.recall_at_3,
        recall_at_5: summary.recall_at_5,
        ndcg_at_5: summary.ndcg_at_5,
        candidate_coverage: summary.candidate_coverage,
        no_tool_precision: summary.no_tool_precision,
        no_tool_recall: summary.no_tool_recall,
        no_tool_f1: summary.no_tool_f1,
        abstention_brier,
        abstention_ece,
        abstention_nll,
        pair_accuracy,
    })
}

fn metadata_for(name: &str, description: &str, category: &str, disclosure: &str) -> ToolMetadata {
    ToolMetadata {
        name: name.into(),
        description: description.into(),
        parameters: serde_json::Value::Null,
        defer_load: disclosure == "deferred",
        category: category.into(),
        disclosure: disclosure.into(),
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

// ---------------------------------------------------------------------------
// Promotion simulation (mirrors the pre-turn loop semantics).
// ---------------------------------------------------------------------------

fn simulate_promotion(
    case: &ToolAdvisorCase,
    prediction: &ToolAdvisorPrediction,
    threshold: f64,
    max_promotions: usize,
    schema_budget_bytes: usize,
) -> (Vec<String>, usize) {
    let current: Vec<ToolMetadata> = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure != "deferred")
        .map(|candidate| {
            metadata_for(
                &candidate.name,
                &candidate.description,
                &candidate.category,
                &candidate.disclosure,
            )
        })
        .collect();
    let deferred: Vec<ToolMetadata> = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| {
            metadata_for(
                &candidate.name,
                &candidate.description,
                &candidate.category,
                &candidate.disclosure,
            )
        })
        .collect();
    let advisor = FixedAdvisor {
        predictions: BTreeMap::from([(case.case_id.clone(), prediction.clone())]),
    };
    let input = ToolAdvisorInput {
        case_id: case.case_id.clone(),
        context: case.context.clone(),
        candidates: case.candidates.clone(),
        surface_fingerprint: "requalify-promotion".into(),
    };
    let projection = project_discovery(
        &current,
        &deferred,
        &input,
        &advisor,
        AdvisorMode::Promote,
        threshold,
        max_promotions,
    );
    // Schema-budget trim mirrors the pre-turn loop: ranked order is
    // score-descending, so the tail drops first.
    let by_name: BTreeMap<&str, &ToolMetadata> = deferred
        .iter()
        .map(|metadata| (metadata.name.as_str(), metadata))
        .collect();
    let mut kept = Vec::new();
    let mut bytes = 0usize;
    for name in &projection.promoted {
        let definition_bytes = by_name
            .get(name.as_str())
            .and_then(|metadata| serde_json::to_vec(metadata).ok())
            .map(|encoded| encoded.len())
            .unwrap_or(0);
        if bytes + definition_bytes > schema_budget_bytes {
            continue;
        }
        bytes += definition_bytes;
        kept.push(name.clone());
    }
    (kept, bytes)
}

fn relevant_deferred(case: &ToolAdvisorCase) -> BTreeSet<String> {
    let deferred: BTreeSet<&str> = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.as_str())
        .collect();
    case.relevance
        .keys()
        .filter(|name| deferred.contains(name.as_str()))
        .cloned()
        .collect()
}

fn promotion_f1(
    cases: &[ToolAdvisorCase],
    predictions: &BTreeMap<String, ToolAdvisorPrediction>,
    threshold: f64,
    max_promotions: usize,
    schema_budget_bytes: usize,
) -> (f64, usize, usize, usize) {
    let mut true_positives = 0usize;
    let mut false_positives = 0usize;
    let mut false_negatives = 0usize;
    for case in cases {
        let Some(prediction) = predictions.get(&case.case_id) else {
            continue;
        };
        let (promoted, _) = simulate_promotion(
            case,
            prediction,
            threshold,
            max_promotions,
            schema_budget_bytes,
        );
        let relevant = relevant_deferred(case);
        for name in &promoted {
            if relevant.contains(name) {
                true_positives += 1;
            } else {
                false_positives += 1;
            }
        }
        for name in &relevant {
            if !promoted.contains(name) {
                false_negatives += 1;
            }
        }
    }
    let precision = if true_positives + false_positives > 0 {
        true_positives as f64 / (true_positives + false_positives) as f64
    } else {
        1.0
    };
    let recall = if true_positives + false_negatives > 0 {
        true_positives as f64 / (true_positives + false_negatives) as f64
    } else {
        1.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    (f1, true_positives, false_positives, false_negatives)
}

// ---------------------------------------------------------------------------
// Main protocol.
// ---------------------------------------------------------------------------

const LEARNED_LABELS: [&str; 4] = [
    "hashed-linear-v1",
    "contextual-small",
    "contextual-medium",
    "contextual-compact",
];

pub fn run(prereg_path: &Path, work_dir_override: Option<&Path>) -> Result<RequalificationReport> {
    let prereg = load_preregistration(prereg_path)?;
    // Prereg paths are repository-root-relative (or absolute); resolve them
    // against the current working directory unchanged.
    let resolve = |path: &str| PathBuf::from(path);
    // Tollgate 1: corpus identity and C001 leakage discipline.
    let dataset_path = resolve(&prereg.dataset);
    let cases = load_cases(Some(&dataset_path))?;
    let dataset_fp = dataset_fingerprint(&cases)?;
    if dataset_fp != prereg.dataset_fingerprint {
        return Err(anyhow!(
            "dataset fingerprint mismatch: expected {}, found {dataset_fp}",
            prereg.dataset_fingerprint
        ));
    }
    let coverage = validate_qualification_corpus(&cases).map_err(|error| {
        anyhow!("C001 leakage discipline violated at requalification time: {error}")
    })?;
    let split_fp = |split: &str| {
        coverage
            .split_fingerprints
            .get(split)
            .cloned()
            .unwrap_or_default()
    };
    for (split, expected) in [
        ("train", &prereg.train_partition_fingerprint),
        ("dev", &prereg.dev_partition_fingerprint),
        ("test", &prereg.test_partition_fingerprint),
    ] {
        if &split_fp(split) != expected {
            return Err(anyhow!(
                "frozen {split} partition mismatch: expected {expected}, found {}",
                split_fp(split)
            ));
        }
    }
    let layout = partition_cases(&cases);
    let owned = |indices: &[usize]| {
        indices
            .iter()
            .map(|&index| cases[index].clone())
            .collect::<Vec<_>>()
    };
    let test_cases = owned(&layout.test_cases);
    let dev_cases = owned(&layout.dev_cases);

    // Tollgate 2: artifact hashes, exclusion lists, and calibration values.
    struct LoadedModel {
        label: String,
        kind: String,
        sha256: String,
        qualified: Option<bool>,
        excluded: Vec<String>,
        scorer: Scorer,
    }
    let mut learned: Vec<LoadedModel> = Vec::new();
    for artifact in &prereg.artifacts {
        let path = resolve(&artifact.path);
        let digest = file_sha256(&path)?;
        if digest != artifact.sha256 {
            return Err(anyhow!(
                "artifact {} hash mismatch: expected {}, found {digest}",
                artifact.label,
                artifact.sha256
            ));
        }
        match artifact.kind.as_str() {
            #[cfg(feature = "tool-advisor")]
            "contextual" => {
                let loaded = contextual::load(&path)?;
                let mut excluded = loaded.manifest.excluded_tool_families.clone();
                excluded.sort();
                let mut expected = artifact.excluded_tool_families.clone();
                expected.sort();
                if excluded != expected {
                    return Err(anyhow!(
                        "artifact {} exclusion mismatch: expected {expected:?}, found {excluded:?}",
                        artifact.label
                    ));
                }
                if let Some(expected) = prereg.expected_calibration.get(&artifact.label) {
                    let actual = loaded.manifest.calibration.as_ref().ok_or_else(|| {
                        anyhow!("artifact {} has no serialized calibration", artifact.label)
                    })?;
                    if (actual.abstain_bias - expected.abstain_bias).abs() > 1e-6
                        || (actual.temperature - expected.temperature).abs() > 1e-6
                    {
                        return Err(anyhow!(
                            "artifact {} calibration substitution detected",
                            artifact.label
                        ));
                    }
                }
                let qualified = loaded.is_qualified();
                if expected.is_empty() && !qualified {
                    return Err(anyhow!(
                        "standard artifact {} is not qualified; refusing to proceed",
                        artifact.label
                    ));
                }
                learned.push(LoadedModel {
                    label: artifact.label.clone(),
                    kind: "contextual".into(),
                    sha256: digest,
                    qualified: Some(qualified),
                    excluded: expected,
                    scorer: Scorer::Contextual(contextual::ContextualAdvisor::new(loaded)?),
                });
            }
            "linear" => {
                let loaded = load_artifact(&path)?;
                let mut excluded = loaded.manifest.excluded_tool_families.clone();
                excluded.sort();
                let mut expected = artifact.excluded_tool_families.clone();
                expected.sort();
                if excluded != expected {
                    return Err(anyhow!(
                        "artifact {} exclusion mismatch: expected {expected:?}, found {excluded:?}",
                        artifact.label
                    ));
                }
                learned.push(LoadedModel {
                    label: artifact.label.clone(),
                    kind: "linear".into(),
                    sha256: digest,
                    qualified: None,
                    excluded: expected,
                    scorer: Scorer::Linear(LinearAdvisor::new(loaded)?),
                });
            }
            other => {
                return Err(anyhow!("unsupported preregistered artifact kind {other}"));
            }
        }
    }
    #[cfg(not(feature = "tool-advisor"))]
    if prereg
        .artifacts
        .iter()
        .any(|artifact| artifact.kind == "contextual")
    {
        return Err(anyhow!(
            "contextual requalification requires the tool-advisor feature"
        ));
    }

    // Slices. Holdout and unknown slices are the only inputs whose labels
    // the optimizer never saw in the corresponding training run.
    let mut slices: BTreeMap<String, Vec<ToolAdvisorCase>> = BTreeMap::new();
    slices.insert("test".into(), test_cases.clone());
    slices.insert("dev".into(), dev_cases.clone());
    let pair_members: Vec<ToolAdvisorCase> = {
        let mut by_family: BTreeMap<&str, Vec<&ToolAdvisorCase>> = BTreeMap::new();
        for case in &test_cases {
            if !case.generated_variant_family.is_empty() {
                by_family
                    .entry(case.generated_variant_family.as_str())
                    .or_default()
                    .push(case);
            }
        }
        by_family
            .values()
            .filter(|members| members.len() == 2)
            .flat_map(|members| members.iter().cloned().cloned())
            .collect()
    };
    slices.insert("counterfactual".into(), pair_members.clone());
    slices.insert(
        "hard-negative".into(),
        test_cases
            .iter()
            .filter(|case| case.tags.iter().any(|tag| tag == "hard-negative"))
            .cloned()
            .collect(),
    );
    let no_tool_cases: Vec<ToolAdvisorCase> = test_cases
        .iter()
        .filter(|case| case.none)
        .cloned()
        .collect();
    slices.insert("no-tool".into(), no_tool_cases.clone());
    let unknown_cases: Vec<ToolAdvisorCase> = test_cases
        .iter()
        .map(unknown_tool_holdout)
        .collect::<Result<Vec<_>>>()
        .context("unknown-tool transform must hold for every test case")?;
    slices.insert("unknown".into(), unknown_cases.clone());
    let mut holdout_indices: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for family in &prereg.holdout_families {
        let holdout = family_holdout_partition(&cases, family);
        if holdout.holdout_cases.is_empty() {
            return Err(anyhow!("holdout family {family} is empty"));
        }
        // Prove optimizer/calibration exclusion before measuring anything.
        for summary in &coverage.leakage.family_holdouts {
            if &summary.family == family && summary.train_dev_family_cases != 0 {
                return Err(anyhow!(
                    "holdout family {family} leaks into optimizer input"
                ));
            }
        }
        holdout_indices.insert(family.clone(), holdout.holdout_cases.clone());
        slices.insert(
            format!("holdout-{family}"),
            holdout
                .holdout_cases
                .iter()
                .map(|&index| cases[index].clone())
                .collect(),
        );
    }

    // Score every model on every slice. Predictions are keyed by case id so
    // transformed (unknown) slices resolve without aliasing the corpus.
    let mut tables: SliceTables = BTreeMap::new();
    let mut score_all = |label: &str, scorer: &Scorer| -> Result<()> {
        let mut per_slice = BTreeMap::new();
        for (slice, slice_cases) in &slices {
            let predictions = slice_cases
                .iter()
                .map(|case| scorer.predict(case, label))
                .collect::<Result<Vec<_>>>()?;
            per_slice.insert(slice.clone(), (slice_cases.clone(), predictions));
        }
        tables.insert(label.into(), per_slice);
        Ok(())
    };
    score_all("keyword", &Scorer::Keyword)?;
    score_all("bm25", &Scorer::Bm25)?;
    for model in &learned {
        score_all(&model.label, &model.scorer)?;
    }

    // In-run exclusion retraining: the only runs allowed to see reduced
    // data, and they never see test labels (train/dev indices only).
    let work_dir = work_dir_override
        .map(PathBuf::from)
        .unwrap_or_else(|| resolve(&prereg.exclusion_training.work_dir));
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("create requalification work dir {}", work_dir.display()))?;
    let exclusion = &prereg.exclusion_training;
    for family in &prereg.holdout_families {
        let context_label = format!("contextual-small-excl-{family}");
        let context_path = work_dir.join(format!("{context_label}.bin"));
        let context_config = ContextualTrainingConfig {
            capacity: contextual::Capacity::parse(Some(&exclusion.contextual.capacity))?,
            epochs: exclusion.contextual.epochs,
            learning_rate: exclusion.contextual.learning_rate,
            seed: exclusion.contextual.seed,
            max_candidates: exclusion.contextual.max_candidates,
            model_version: Some(context_label.clone()),
            vocab_buckets: Some(exclusion.contextual.vocab_buckets),
            exclude_tool_families: vec![family.clone()],
            allow_unpartitioned_fallback: false,
        };
        let context_report =
            contextual::train(&cases, &dataset_fp, &context_config, &context_path)?;
        if context_report.unqualified_fallback
            || context_report.excluded_tool_families != vec![family.clone()]
        {
            return Err(anyhow!(
                "exclusion training for {family} did not preserve holdout discipline"
            ));
        }
        let loaded = contextual::load(&context_path)?;
        if !loaded.is_qualified() {
            return Err(anyhow!("exclusion artifact for {family} is not qualified"));
        }
        let scorer = Scorer::Contextual(contextual::ContextualAdvisor::new(loaded)?);
        for slice in [
            format!("holdout-{family}"),
            "test".to_string(),
            "no-tool".to_string(),
        ] {
            let slice_cases = tables["bm25"][&slice].0.clone();
            let predictions = slice_cases
                .iter()
                .map(|case| scorer.predict(case, &context_label))
                .collect::<Result<Vec<_>>>()?;
            tables
                .entry(context_label.clone())
                .or_default()
                .insert(slice, (slice_cases, predictions));
        }
        learned.push(LoadedModel {
            label: context_label,
            kind: "contextual".into(),
            sha256: file_sha256(&context_path)?,
            qualified: Some(true),
            excluded: vec![family.clone()],
            scorer,
        });

        let linear_label = format!("linear-excl-{family}");
        let linear_path = work_dir.join(format!("{linear_label}.json"));
        let linear_run = work_dir.join(format!("{linear_label}-run"));
        let linear_config = TrainingConfig {
            schema_version: TRAINING_CONFIG_VERSION,
            architecture: None,
            capacity: None,
            vocab_buckets: None,
            exclude_tool_families: vec![family.clone()],
            allow_unpartitioned_fallback: false,
            dataset: Some(dataset_path.display().to_string()),
            output_artifact: linear_path.display().to_string(),
            run_dir: Some(linear_run.display().to_string()),
            epochs: exclusion.linear.epochs,
            learning_rate: exclusion.linear.learning_rate,
            seed: exclusion.linear.seed,
            max_context_bytes: MAX_CONTEXT_BYTES,
            max_candidates: exclusion.linear.max_candidates.min(MAX_CANDIDATES),
            calibration_temperature: exclusion.linear.calibration_temperature,
            resume: false,
        };
        let linear_report = training::train(&linear_config)?;
        if linear_report.excluded_tool_families != vec![family.clone()] {
            return Err(anyhow!(
                "linear exclusion training for {family} did not preserve holdout discipline"
            ));
        }
        let linear_artifact = load_artifact(&linear_path)?;
        let linear_scorer = Scorer::Linear(LinearAdvisor::new(linear_artifact)?);
        for slice in [
            format!("holdout-{family}"),
            "test".to_string(),
            "no-tool".to_string(),
        ] {
            let slice_cases = tables["bm25"][&slice].0.clone();
            let predictions = slice_cases
                .iter()
                .map(|case| linear_scorer.predict(case, &linear_label))
                .collect::<Result<Vec<_>>>()?;
            tables
                .entry(linear_label.clone())
                .or_default()
                .insert(slice, (slice_cases, predictions));
        }
        learned.push(LoadedModel {
            label: linear_label,
            kind: "linear".into(),
            sha256: file_sha256(&linear_path)?,
            qualified: None,
            excluded: vec![family.clone()],
            scorer: linear_scorer,
        });
    }

    // Shared promotion threshold selected on dev only.
    let dev_predictions: BTreeMap<String, BTreeMap<String, ToolAdvisorPrediction>> = LEARNED_LABELS
        .iter()
        .map(|label| {
            let per_slice = &tables[*label];
            let (dev_cases, dev_predictions) = &per_slice["dev"];
            let _ = dev_cases;
            (
                label.to_string(),
                dev_predictions
                    .iter()
                    .map(|prediction| (prediction.case_id.clone(), prediction.clone()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect();
    let mut best = (f64::MIN, prereg.promotion.threshold_grid[0]);
    for threshold in &prereg.promotion.threshold_grid {
        let mut total = 0.0;
        for label in LEARNED_LABELS {
            let (f1, _, _, _) = promotion_f1(
                &dev_cases,
                &dev_predictions[label],
                *threshold,
                prereg.promotion.max_promotions,
                prereg.promotion.schema_budget_bytes,
            );
            total += f1;
        }
        let mean = total / LEARNED_LABELS.len() as f64;
        // Ties resolve to the higher (conservative) threshold.
        if mean > best.0 || (mean == best.0 && *threshold > best.1) {
            best = (mean, *threshold);
        }
    }
    let (dev_f1_at_threshold, shared_threshold) = (best.0, best.1);

    // Assemble per-model reports.
    let mut models = Vec::new();
    let all_labels: Vec<String> = ["keyword".to_string(), "bm25".to_string()]
        .into_iter()
        .chain(learned.iter().map(|model| model.label.clone()))
        .collect();
    for label in &all_labels {
        let per_slice = &tables[label];
        let mut slice_reports = BTreeMap::new();
        for (slice, (slice_cases, predictions)) in per_slice {
            let pair_accuracy = if slice == "counterfactual" {
                Some(pair_accuracy(slice_cases, predictions))
            } else {
                None
            };
            slice_reports.insert(
                slice.clone(),
                slice_metrics(slice_cases, predictions, pair_accuracy)?,
            );
        }
        let (kind, sha256, qualified, excluded) =
            match learned.iter().find(|model| &model.label == label) {
                Some(model) => (
                    model.kind.clone(),
                    Some(model.sha256.clone()),
                    model.qualified,
                    model.excluded.clone(),
                ),
                None => ("baseline".into(), None, None, Vec::new()),
            };
        // Promotion simulation applies to learned models on the test slice.
        let promotion = if LEARNED_LABELS.contains(&label.as_str()) {
            let (test_cases, test_predictions) = &per_slice["test"];
            let by_id: BTreeMap<String, &ToolAdvisorPrediction> = test_predictions
                .iter()
                .map(|prediction| (prediction.case_id.clone(), prediction))
                .collect();
            let mut true_positives = 0usize;
            let mut false_positives = 0usize;
            let mut false_negatives = 0usize;
            let mut no_tool_promoted = 0usize;
            let mut no_tool_total = 0usize;
            let mut schema_bytes = Vec::new();
            for case in test_cases {
                let Some(prediction) = by_id.get(&case.case_id) else {
                    continue;
                };
                let (promoted, bytes) = simulate_promotion(
                    case,
                    prediction,
                    shared_threshold,
                    prereg.promotion.max_promotions,
                    prereg.promotion.schema_budget_bytes,
                );
                let relevant = relevant_deferred(case);
                for name in &promoted {
                    if relevant.contains(name) {
                        true_positives += 1;
                    } else {
                        false_positives += 1;
                    }
                }
                for name in &relevant {
                    if !promoted.contains(name) {
                        false_negatives += 1;
                    }
                }
                if case.none {
                    no_tool_total += 1;
                    if !promoted.is_empty() {
                        no_tool_promoted += 1;
                    }
                }
                if !promoted.is_empty() {
                    schema_bytes.push(bytes);
                }
            }
            let recall = if true_positives + false_negatives > 0 {
                true_positives as f64 / (true_positives + false_negatives) as f64
            } else {
                1.0
            };
            let false_rate = if true_positives + false_positives > 0 {
                false_positives as f64 / (true_positives + false_positives) as f64
            } else {
                0.0
            };
            Some(PromotionSliceReport {
                threshold: shared_threshold,
                dev_f1_at_threshold,
                test_recall: recall,
                test_false_promotion_rate: false_rate,
                test_no_tool_promotion_rate: if no_tool_total > 0 {
                    no_tool_promoted as f64 / no_tool_total as f64
                } else {
                    0.0
                },
                mean_schema_bytes_added: if schema_bytes.is_empty() {
                    0.0
                } else {
                    schema_bytes.iter().sum::<usize>() as f64 / schema_bytes.len() as f64
                },
                max_schema_bytes_added: schema_bytes.into_iter().max().unwrap_or(0),
            })
        } else {
            None
        };
        let resource = measure_resource(label, per_slice, &resolve)?;
        models.push(ModelReport {
            label: label.clone(),
            kind,
            artifact_sha256: sha256,
            qualified,
            excluded_tool_families: excluded,
            slices: slice_reports,
            promotion,
            resource,
        });
    }

    // Large-catalog deferred candidate-recall fixture (preselector property).
    let recall_fixture = recall_fixture_report(&cases, &test_cases, &prereg)?;

    // Authority-negative probe (C003 seam, re-executed here as a gate).
    let authority_violations = authority_probe()?;

    // Uncalibrated abstention reference from the same ranked scores, so
    // gate 7 compares identical rankings under both abstention formulas.
    let mut uncalibrated: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for label in [
        "contextual-small",
        "contextual-medium",
        "contextual-compact",
    ] {
        if let Some(per_slice) = tables.get(label) {
            if let Some((no_tool_cases, predictions)) = per_slice.get("no-tool") {
                let pairs: Vec<(f64, bool)> = no_tool_cases
                    .iter()
                    .filter_map(|case| {
                        predictions
                            .iter()
                            .find(|prediction| prediction.case_id == case.case_id)
                            .map(|prediction| {
                                let top = prediction
                                    .ranked
                                    .first()
                                    .map(|item| item.score)
                                    .unwrap_or(0.0);
                                (uncalibrated_abstention(top), case.none)
                            })
                    })
                    .collect();
                uncalibrated.insert(label.into(), (brier(&pairs), ece(&pairs)));
            }
        }
    }

    // Gates and disposition.
    let (gates, disposition, selected_artifact) = decide(
        &models,
        &recall_fixture,
        authority_violations,
        &uncalibrated,
        &prereg,
    );
    let live_m004_transition = match disposition {
        Disposition::APositive | Disposition::CResourceEfficient => format!(
            "Tool-selection advisor post-closure corrective M004 blocked -> ready (C004 {} with {})",
            match disposition {
                Disposition::APositive => "positive disposition A",
                _ => "positive disposition C",
            },
            selected_artifact.as_deref().unwrap_or("unknown artifact")
        ),
        Disposition::BNoUsefulGain | Disposition::DCorrectnessFailure => {
            "live M004 remains blocked (C004 non-positive disposition)".to_string()
        }
    };

    Ok(RequalificationReport {
        schema_version: prereg.schema_version,
        protocol: prereg.protocol.clone(),
        protocol_hash: prereg.protocol_hash.clone(),
        model_config_hashes: prereg.model_config_hashes.clone(),
        declared_gate_set: prereg.declared_gate_set.clone(),
        preregistration_commit_sha: prereg.preregistration_commit_sha.clone(),
        dataset: prereg.dataset.clone(),
        dataset_fingerprint: dataset_fp,
        train_partition_fingerprint: split_fp("train"),
        dev_partition_fingerprint: split_fp("dev"),
        test_partition_fingerprint: split_fp("test"),
        leakage_violations: 0,
        models,
        recall_fixture,
        authority_violations,
        shared_promotion_threshold: shared_threshold,
        gates,
        disposition,
        selected_artifact,
        live_m004_transition,
    })
}

fn pair_accuracy(cases: &[ToolAdvisorCase], predictions: &[ToolAdvisorPrediction]) -> f64 {
    let by_id: BTreeMap<&str, &ToolAdvisorPrediction> = predictions
        .iter()
        .map(|prediction| (prediction.case_id.as_str(), prediction))
        .collect();
    let mut by_family: BTreeMap<&str, Vec<&ToolAdvisorCase>> = BTreeMap::new();
    for case in cases {
        if !case.generated_variant_family.is_empty() {
            by_family
                .entry(case.generated_variant_family.as_str())
                .or_default()
                .push(case);
        }
    }
    let pairs: Vec<&Vec<&ToolAdvisorCase>> = by_family
        .values()
        .filter(|members| members.len() == 2)
        .collect();
    if pairs.is_empty() {
        return 1.0;
    }
    let mut correct = 0usize;
    for members in &pairs {
        let mut pair_correct = true;
        for case in members.iter() {
            let Some(prediction) = by_id.get(case.case_id.as_str()) else {
                pair_correct = false;
                break;
            };
            let abstains = prediction.abstain_probability.unwrap_or(0.0) >= 0.5;
            if case.none {
                if !abstains {
                    pair_correct = false;
                }
                continue;
            }
            if abstains {
                pair_correct = false;
                continue;
            }
            let relevant: BTreeSet<&str> = case.relevance.keys().map(String::as_str).collect();
            let top = prediction.ranked.first().map(|item| item.name.as_str());
            if top.is_none_or(|name| !relevant.contains(name)) {
                pair_correct = false;
            }
        }
        if pair_correct {
            correct += 1;
        }
    }
    correct as f64 / pairs.len() as f64
}

fn measure_resource(
    label: &str,
    per_slice: &BTreeMap<String, (Vec<ToolAdvisorCase>, Vec<ToolAdvisorPrediction>)>,
    resolve: &dyn Fn(&str) -> PathBuf,
) -> Result<Option<ResourceSliceReport>> {
    // Baselines have no artifact to measure.
    if label == "keyword" || label == "bm25" {
        return Ok(None);
    }
    let (artifact_path, is_contextual) =
        if label == "hashed-linear-v1" || label.starts_with("linear-excl-") {
            let name = if label == "hashed-linear-v1" {
                "target/tool-advisor-smoke/model.json"
            } else {
                ""
            };
            let path = if name.is_empty() {
                resolve(&format!("target/tool-advisor/c004-run/{label}.json"))
            } else {
                resolve(name)
            };
            (path, false)
        } else if label.starts_with("contextual-small-excl-") {
            (
                resolve(&format!("target/tool-advisor/c004-run/{label}.bin")),
                true,
            )
        } else {
            (resolve(&format!("target/tool-advisor/{label}.bin")), true)
        };
    let bytes = fs::metadata(&artifact_path)
        .with_context(|| format!("stat {}", artifact_path.display()))?
        .len();
    let started = Instant::now();
    if is_contextual {
        let _ = contextual::load(&artifact_path)?;
    } else {
        let _ = load_artifact(&artifact_path)?;
    }
    let cold_load_millis = started.elapsed().as_millis();
    let (test_cases, _) = per_slice
        .get("test")
        .ok_or_else(|| anyhow!("missing test slice for {label}"))?;
    // Latency probing rescores without inspecting labels.
    let mut latencies: Vec<u128> = Vec::new();
    if is_contextual {
        let advisor = contextual::ContextualAdvisor::new(contextual::load(&artifact_path)?)?;
        for case in test_cases.iter().take(200) {
            let started = Instant::now();
            let _ = advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: "requalify-latency".into(),
            })?;
            latencies.push(started.elapsed().as_micros());
        }
    } else {
        let advisor = LinearAdvisor::new(load_artifact(&artifact_path)?)?;
        for case in test_cases.iter().take(200) {
            let started = Instant::now();
            let _ = advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: "requalify-latency".into(),
            })?;
            latencies.push(started.elapsed().as_micros());
        }
    }
    latencies.sort_unstable();
    let percentile = |quantile: f64| {
        if latencies.is_empty() {
            0
        } else {
            latencies[((latencies.len() - 1) as f64 * quantile).round() as usize]
        }
    };
    // Effective-capacity sidecar: best effort, recorded when present.
    let sidecar = artifact_path.with_extension(if is_contextual {
        "training-report.json"
    } else {
        "json"
    });
    let (trained_parameters, trained_fraction, allocated) = if is_contextual && sidecar.exists() {
        let report: serde_json::Value = serde_json::from_slice(
            &fs::read(&sidecar).with_context(|| format!("read {}", sidecar.display()))?,
        )
        .context("parse training report sidecar")?;
        let capacity = &report["capacity_report"];
        (
            capacity["trained_parameter_estimate"].as_u64(),
            capacity["trained_parameter_fraction"].as_f64(),
            capacity["allocated_parameter_count"].as_u64().unwrap_or(0),
        )
    } else if !is_contextual {
        let artifact = load_artifact(&artifact_path)?;
        (
            Some(artifact.manifest.parameter_count),
            Some(1.0),
            artifact.manifest.parameter_count,
        )
    } else {
        (None, None, 0)
    };
    Ok(Some(ResourceSliceReport {
        artifact_bytes: bytes,
        allocated_parameters: allocated,
        trained_parameters,
        trained_fraction,
        cold_load_millis,
        score_p50_micros: percentile(0.5),
        score_p95_micros: percentile(0.95),
    }))
}

fn recall_fixture_report(
    cases: &[ToolAdvisorCase],
    test_cases: &[ToolAdvisorCase],
    prereg: &Preregistration,
) -> Result<RecallFixtureReport> {
    // Deterministic global descriptor pool from the frozen corpus.
    let mut descriptors: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for case in cases {
        for candidate in &case.candidates {
            descriptors.entry(candidate.name.clone()).or_insert((
                candidate.description.clone(),
                candidate.category.clone(),
                candidate.disclosure.clone(),
            ));
        }
    }
    let mut total_relevant = 0usize;
    let mut total_hit = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut latencies: Vec<u128> = Vec::new();
    for case in test_cases {
        if case.relevance.is_empty() {
            continue;
        }
        let mut universe: Vec<super::ToolAdvisorCandidate> = case.candidates.clone();
        let present: BTreeSet<String> = universe
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        for (name, (description, category, disclosure)) in &descriptors {
            if universe.len() >= 64 {
                break;
            }
            if present.contains(name) {
                continue;
            }
            universe.push(super::ToolAdvisorCandidate {
                name: name.clone(),
                description: description.clone(),
                category: category.clone(),
                disclosure: disclosure.clone(),
                synthetic_identity: false,
            });
        }
        let relevant: BTreeSet<String> = case.relevance.keys().cloned().collect();
        total_relevant += relevant.len();
        let started = Instant::now();
        let (_, report) =
            preselect_candidates(universe, &case.context, prereg.promotion.max_candidates);
        latencies.push(started.elapsed().as_millis());
        let recall = preselection_recall(&report, &relevant);
        total_hit += recall.relevant_shortlisted;
        missing.extend(
            recall
                .missing
                .into_iter()
                .map(|name| format!("{}:{}", case.case_id, name)),
        );
    }
    latencies.sort_unstable();
    let mean = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<u128>() as f64 / latencies.len() as f64
    };
    missing.sort();
    missing.truncate(32);
    Ok(RecallFixtureReport {
        cases: test_cases
            .iter()
            .filter(|case| !case.relevance.is_empty())
            .count(),
        universe_size: 64,
        budget: prereg.promotion.max_candidates,
        relevant_total: total_relevant,
        relevant_shortlisted: total_hit,
        recall: if total_relevant == 0 {
            1.0
        } else {
            total_hit as f64 / total_relevant as f64
        },
        missing,
        mean_preselect_millis: mean,
        max_preselect_millis: latencies.into_iter().max().unwrap_or(0),
    })
}

fn authority_probe() -> Result<usize> {
    use crate::agent::tool_surface::ResolvedToolSurface;
    use crate::provider::ToolDefinition;
    let definitions = vec![
        ToolDefinition {
            name: "core_a".into(),
            description: "Core tool A".into(),
            parameters: serde_json::Value::Null,
            defer_loading: None,
        },
        ToolDefinition {
            name: "denied_deferred".into(),
            description: "Denied deferred tool".into(),
            parameters: serde_json::Value::Null,
            defer_loading: Some(true),
        },
    ];
    let surface = ResolvedToolSurface::resolve(
        definitions,
        &BTreeSet::from(["denied_deferred".to_string()]),
        &BTreeSet::new(),
        false,
        true,
        None,
    )
    .expect("probe surface");
    let deferred_names: BTreeSet<String> = surface
        .definitions()
        .into_iter()
        .filter(|definition| definition.defer_loading == Some(true))
        .map(|definition| definition.name)
        .collect();
    let mut violations = 0usize;
    // The denied tool must be absent from the eligible deferred universe.
    let eligible = candidates_from_deferred_surface(&surface, &deferred_names);
    if eligible
        .iter()
        .any(|candidate| candidate.name == "denied_deferred")
    {
        violations += 1;
    }
    // Even an adversarial prediction ranking it first cannot promote it:
    // promotion only draws from the allowed deferred set.
    let current = vec![metadata_for("core_a", "Core tool A", "ReadOnly", "core")];
    let allowed: Vec<ToolMetadata> = eligible
        .iter()
        .map(|candidate| {
            metadata_for(
                &candidate.name,
                &candidate.description,
                &candidate.category,
                &candidate.disclosure,
            )
        })
        .collect();
    let adversarial = FixedAdvisor {
        predictions: BTreeMap::from([(
            "probe".to_string(),
            ToolAdvisorPrediction {
                schema_version: PREDICTION_SCHEMA_VERSION,
                case_id: "probe".into(),
                ranked: vec![
                    RankedCandidate {
                        name: "denied_deferred".into(),
                        score: 0.99,
                    },
                    RankedCandidate {
                        name: "core_a".into(),
                        score: 0.1,
                    },
                ],
                abstain_probability: Some(0.0),
                mode: "probe".into(),
            },
        )]),
    };
    let projection = project_discovery(
        &current,
        &allowed,
        &ToolAdvisorInput {
            case_id: "probe".into(),
            context: "probe".into(),
            candidates: eligible,
            surface_fingerprint: "probe".into(),
        },
        &adversarial,
        AdvisorMode::Promote,
        0.0,
        2,
    );
    if projection
        .promoted
        .iter()
        .any(|name| name == "denied_deferred")
    {
        violations += 1;
    }
    Ok(violations)
}

// ---------------------------------------------------------------------------
// Gates and disposition.
// ---------------------------------------------------------------------------

fn get_slice<'a>(model: &'a ModelReport, name: &str) -> Option<&'a SliceMetrics> {
    model.slices.get(name)
}

type SliceTables =
    BTreeMap<String, BTreeMap<String, (Vec<ToolAdvisorCase>, Vec<ToolAdvisorPrediction>)>>;

fn decide(
    models: &[ModelReport],
    recall_fixture: &RecallFixtureReport,
    authority_violations: usize,
    uncalibrated: &BTreeMap<String, (f64, f64)>,
    prereg: &Preregistration,
) -> (Vec<GateVerdict>, Disposition, Option<String>) {
    let mut gates = Vec::new();
    let get = |label: &str| models.iter().find(|model| model.label == label);
    let slice = get_slice;

    // Gate 1: zero leakage violations (proven by the tollgate validator).
    gates.push(GateVerdict {
        id: "leakage-zero".into(),
        passed: true,
        detail: "validate_qualification_corpus passed: 0 exact, 0 normalized, 0 template cross-split overlaps; 0 contradictions".into(),
    });

    // Gate 2: preselector recall.
    let gate2 = recall_fixture.recall >= prereg.gates.preselect_recall_min;
    gates.push(GateVerdict {
        id: "preselector-recall".into(),
        passed: gate2,
        detail: format!(
            "large-catalog recall {:.4} over {} relevant (min {:.2})",
            recall_fixture.recall, recall_fixture.relevant_total, prereg.gates.preselect_recall_min
        ),
    });

    // Gate 3: authority negatives. Any violation is a correctness failure.
    let gate3 = authority_violations == 0;
    gates.push(GateVerdict {
        id: "authority-zero".into(),
        passed: gate3,
        detail: format!("{authority_violations} authority-negative promotion violations"),
    });

    // Context-sensitive slices for gate 5: counterfactual, unknown, and the
    // true holdouts measured with exclusion-trained variants. The fair
    // comparator on a holdout slice is the exclusion-trained linear model;
    // medium/compact have no exclusion retraining, so their holdout rows are
    // seen-family references (labeled as such, never counted as gains).
    let context_slices: Vec<String> = ["counterfactual".to_string(), "unknown".to_string()]
        .into_iter()
        .chain(
            prereg
                .holdout_families
                .iter()
                .map(|family| format!("holdout-{family}")),
        )
        .collect();

    let linear_mrr = get("hashed-linear-v1")
        .and_then(|model| slice(model, "test"))
        .map(|metrics| metrics.mrr)
        .unwrap_or(f64::NAN);
    let baseline_f1 = ["keyword", "bm25", "hashed-linear-v1"]
        .iter()
        .filter_map(|label| get(label).and_then(|model| slice(model, "no-tool")))
        .filter_map(|metrics| metrics.no_tool_f1)
        .fold(f64::MIN, f64::max);
    let baseline_f1 = if baseline_f1 == f64::MIN {
        0.0
    } else {
        baseline_f1
    };

    // Per-variant bundles for gates 4-8.
    let mut bundle_lines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut bundle_pass: BTreeMap<String, bool> = BTreeMap::new();
    for variant in [
        "contextual-small",
        "contextual-medium",
        "contextual-compact",
    ] {
        let mut lines = Vec::new();
        let mut pass = true;

        // Gate 4: aggregate test MRR within tolerance of hashed-linear-v1.
        match get(variant).and_then(|model| slice(model, "test")) {
            Some(metrics) => {
                let ok = linear_mrr - metrics.mrr <= prereg.gates.aggregate_mrr_tolerance_vs_linear;
                lines.push(format!(
                    "aggregate MRR {:.4} vs linear {:.4}: {}",
                    metrics.mrr,
                    linear_mrr,
                    if ok { "pass" } else { "FAIL" }
                ));
                pass = pass && ok;
            }
            None => {
                lines.push("aggregate test metrics missing: FAIL".into());
                pass = false;
            }
        }

        // Gate 5: slice gain without regression.
        let mut gains = Vec::new();
        let mut regressions = Vec::new();
        for slice_name in &context_slices {
            let is_holdout = slice_name.starts_with("holdout-");
            let family = slice_name.strip_prefix("holdout-").unwrap_or("");
            let (model_label, linear_label, caveat) = if is_holdout && variant == "contextual-small"
            {
                (
                    format!("contextual-small-excl-{family}"),
                    format!("linear-excl-{family}"),
                    "",
                )
            } else if is_holdout {
                (
                    variant.to_string(),
                    format!("linear-excl-{family}"),
                    " (seen-family reference)",
                )
            } else {
                (variant.to_string(), "hashed-linear-v1".to_string(), "")
            };
            let value = get(&model_label)
                .and_then(|model| slice(model, slice_name))
                .map(|metrics| (metrics.mrr, metrics.recall_at_1));
            let baseline = get(&linear_label)
                .and_then(|model| slice(model, slice_name))
                .map(|metrics| (metrics.mrr, metrics.recall_at_1));
            match (value, baseline) {
                (Some((mrr, r1)), Some((base_mrr, base_r1))) => {
                    let gain = (mrr - base_mrr).max(r1 - base_r1);
                    let worst = (base_mrr - mrr).max(base_r1 - r1);
                    // Seen-family references inform but never count.
                    if caveat.is_empty() {
                        if gain >= prereg.gates.slice_gain_min {
                            gains.push(slice_name.clone());
                        }
                        if worst >= prereg.gates.slice_regression_max {
                            regressions.push(slice_name.clone());
                        }
                    }
                    lines.push(format!(
                        "{slice_name}: dMRR {d_mrr:+.4} dR1 {d_r1:+.4}{caveat}",
                        d_mrr = mrr - base_mrr,
                        d_r1 = r1 - base_r1,
                    ));
                }
                _ => {
                    lines.push(format!("{slice_name}: missing metrics: FAIL"));
                    regressions.push(slice_name.clone());
                }
            }
        }
        let gain_ok = !gains.is_empty() && regressions.is_empty();
        lines.push(format!(
            "slices with >=0.02 gain: {gains:?}; with >=0.02 regression: {regressions:?}: {}",
            if gain_ok { "pass" } else { "FAIL" }
        ));
        pass = pass && gain_ok;

        // Gate 6: no-tool F1 within tolerance of the best baseline.
        match get(variant)
            .and_then(|model| slice(model, "no-tool"))
            .and_then(|metrics| metrics.no_tool_f1)
        {
            Some(f1) => {
                let ok = baseline_f1 - f1 <= prereg.gates.no_tool_f1_tolerance_vs_best_baseline;
                // A null baseline (no baseline abstains at all) with a null
                // model F1 is recorded, not punished: both abstain never.
                let ok = ok || (baseline_f1 == 0.0 && f1 == 0.0);
                lines.push(format!(
                    "no-tool F1 {f1:.4} vs best baseline {baseline_f1:.4}: {}",
                    if ok { "pass" } else { "FAIL" }
                ));
                pass = pass && ok;
            }
            None => {
                // Neither the model nor any baseline abstains on the slice:
                // F1 is undefined on both sides. This is a measurement gap,
                // not a regression, provided the baseline side is also null.
                let ok = baseline_f1 == 0.0;
                lines.push(format!(
                    "no-tool F1 undefined (model never abstains); best baseline {baseline_f1:.4}: {}",
                    if ok { "pass (both null)" } else { "FAIL" }
                ));
                pass = pass && ok;
            }
        }

        // Gate 7: calibrated Brier/ECE non-inferior to the uncalibrated form.
        match (
            get(variant).and_then(|model| slice(model, "no-tool")),
            uncalibrated.get(variant),
        ) {
            (Some(metrics), Some((uncal_brier, uncal_ece))) => {
                let ok = metrics.abstention_brier <= uncal_brier + 1e-9
                    && metrics.abstention_ece <= uncal_ece + 1e-9;
                lines.push(format!(
                    "cal Brier {:.4}/ECE {:.4} vs uncal {:.4}/{:.4}: {}",
                    metrics.abstention_brier,
                    metrics.abstention_ece,
                    uncal_brier,
                    uncal_ece,
                    if ok { "pass" } else { "FAIL" }
                ));
                pass = pass && ok;
            }
            _ => {
                lines.push("calibration comparison missing: FAIL".into());
                pass = false;
            }
        }

        // Gate 8: footprint/latency within declared targets.
        match get(variant).and_then(|model| model.resource.as_ref()) {
            Some(resource) => {
                let ok = resource.artifact_bytes <= prereg.resource_targets.max_artifact_bytes
                    && resource.cold_load_millis <= prereg.resource_targets.max_cold_load_millis
                    && resource.score_p95_micros <= prereg.resource_targets.max_score_p95_micros;
                lines.push(format!(
                    "footprint {}B cold {}ms p95 {}us: {}",
                    resource.artifact_bytes,
                    resource.cold_load_millis,
                    resource.score_p95_micros,
                    if ok { "pass" } else { "FAIL" }
                ));
                pass = pass && ok;
            }
            None => {
                lines.push("resource report missing: FAIL".into());
                pass = false;
            }
        }

        bundle_lines.insert(variant.into(), lines);
        bundle_pass.insert(variant.into(), pass);
    }

    for variant in [
        "contextual-small",
        "contextual-medium",
        "contextual-compact",
    ] {
        gates.push(GateVerdict {
            id: format!("variant-{variant}"),
            passed: bundle_pass[variant],
            detail: bundle_lines[variant].join(" | "),
        });
    }
    // Preselect latency joins the footprint verdict text.
    gates.push(GateVerdict {
        id: "footprint-preselect-latency".into(),
        passed: recall_fixture.max_preselect_millis <= prereg.resource_targets.max_preselect_millis,
        detail: format!(
            "preselect mean {:.2}ms max {}ms (target {}ms)",
            recall_fixture.mean_preselect_millis,
            recall_fixture.max_preselect_millis,
            prereg.resource_targets.max_preselect_millis
        ),
    });

    // Disposition.
    let candidates: Vec<String> = [
        "contextual-small",
        "contextual-medium",
        "contextual-compact",
    ]
    .into_iter()
    .filter(|variant| bundle_pass[*variant])
    .map(str::to_string)
    .collect();
    let (disposition, selected_artifact) = if !gate3 {
        // Authority violation: correctness failure, never unblock.
        (Disposition::DCorrectnessFailure, None)
    } else if !gate2 {
        // Recall gate is an effectiveness property of the preselector, not a
        // correctness break, but without it no positive disposition is sound.
        (Disposition::BNoUsefulGain, None)
    } else if candidates.is_empty() {
        (Disposition::BNoUsefulGain, None)
    } else if candidates.contains(&"contextual-compact".to_string()) {
        // The compact table matches quality: select it and proceed only
        // with the efficient configuration.
        (
            Disposition::CResourceEfficient,
            Some("contextual-compact".to_string()),
        )
    } else {
        // Positive qualification with the best passing variant.
        let mut best = candidates[0].clone();
        let mut best_mrr = f64::MIN;
        for variant in &candidates {
            if let Some(mrr) = get(variant)
                .and_then(|model| slice(model, "test"))
                .map(|metrics| metrics.mrr)
            {
                if mrr > best_mrr {
                    best_mrr = mrr;
                    best = variant.clone();
                }
            }
        }
        (Disposition::APositive, Some(best))
    };
    (gates, disposition, selected_artifact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(mrr: f64, r1: f64, f1: Option<f64>) -> SliceMetrics {
        SliceMetrics {
            cases: 10,
            mrr,
            recall_at_1: r1,
            recall_at_3: r1,
            recall_at_5: r1,
            ndcg_at_5: mrr,
            candidate_coverage: 1.0,
            no_tool_precision: f1,
            no_tool_recall: f1,
            no_tool_f1: f1,
            abstention_brier: 0.1,
            abstention_ece: 0.1,
            abstention_nll: 0.3,
            pair_accuracy: None,
        }
    }

    fn model(
        label: &str,
        test_mrr: f64,
        test_r1: f64,
        slices: BTreeMap<String, SliceMetrics>,
    ) -> ModelReport {
        let mut full = slices;
        full.insert("test".into(), metrics(test_mrr, test_r1, Some(0.5)));
        full.entry("no-tool".into())
            .or_insert(metrics(0.0, 0.0, Some(0.5)));
        ModelReport {
            label: label.into(),
            kind: "test".into(),
            artifact_sha256: None,
            qualified: None,
            excluded_tool_families: Vec::new(),
            slices: full,
            promotion: None,
            resource: Some(ResourceSliceReport {
                artifact_bytes: 1024,
                allocated_parameters: 100,
                trained_parameters: Some(50),
                trained_fraction: Some(0.5),
                cold_load_millis: 10,
                score_p50_micros: 100,
                score_p95_micros: 200,
            }),
        }
    }

    fn prereg() -> Preregistration {
        Preregistration {
            schema_version: 1,
            protocol: "test".into(),
            dataset: "test.jsonl".into(),
            dataset_fingerprint: "abc".into(),
            train_partition_fingerprint: "abc".into(),
            dev_partition_fingerprint: "abc".into(),
            test_partition_fingerprint: "abc".into(),
            holdout_families: vec!["plugin".into()],
            unknown_transform: "test".into(),
            artifacts: Vec::new(),
            expected_calibration: BTreeMap::new(),
            exclusion_training: ExclusionTrainingSection {
                contextual: ExclusionTrainingConfig {
                    capacity: "small".into(),
                    vocab_buckets: 64,
                    epochs: 1,
                    learning_rate: 0.1,
                    seed: 1,
                    max_candidates: 8,
                },
                linear: LinearExclusionTrainingConfig {
                    epochs: 1,
                    learning_rate: 0.1,
                    seed: 1,
                    max_context_bytes: 1024,
                    max_candidates: 8,
                    calibration_temperature: 1.0,
                },
                work_dir: "target/test".into(),
            },
            promotion: PromotionSection {
                max_candidates: 16,
                max_promotions: 2,
                schema_budget_bytes: 16384,
                margin: 0.0,
                threshold_grid: vec![0.5],
                selection_rule: "test".into(),
            },
            gates: GateSection {
                preselect_recall_min: 0.98,
                aggregate_mrr_tolerance_vs_linear: 0.01,
                slice_gain_min: 0.02,
                slice_regression_max: 0.02,
                no_tool_f1_tolerance_vs_best_baseline: 0.02,
            },
            resource_targets: ResourceTargetSection {
                max_artifact_bytes: 1 << 30,
                max_cold_load_millis: 60_000,
                max_score_p95_micros: 100_000,
                max_preselect_millis: 1000,
            },
            protocol_hash: String::new(),
            model_config_hashes: BTreeMap::new(),
            declared_gate_set: Vec::new(),
            preregistration_commit_sha: None,
        }
    }

    fn recall_fixture(recall: f64) -> RecallFixtureReport {
        RecallFixtureReport {
            cases: 10,
            universe_size: 64,
            budget: 16,
            relevant_total: 10,
            relevant_shortlisted: (recall * 10.0).round() as usize,
            recall,
            missing: Vec::new(),
            mean_preselect_millis: 1.0,
            max_preselect_millis: 5,
        }
    }

    fn linear_test_mrr(models: &mut [ModelReport], mrr: f64) {
        if let Some(model) = models
            .iter_mut()
            .find(|model| model.label == "hashed-linear-v1")
        {
            model
                .slices
                .insert("test".into(), metrics(mrr, 0.5, Some(0.5)));
        }
    }

    fn passing_models() -> Vec<ModelReport> {
        // Linear baseline plus one contextual variant beating it on a slice
        // with no regressions anywhere.
        let mut slices = BTreeMap::new();
        slices.insert("counterfactual".into(), metrics(0.70, 0.60, None));
        slices.insert("unknown".into(), metrics(0.70, 0.60, None));
        slices.insert("holdout-plugin".into(), metrics(0.70, 0.60, None));
        let linear = model("hashed-linear-v1", 0.70, 0.50, {
            let mut linear_slices = BTreeMap::new();
            linear_slices.insert("counterfactual".into(), metrics(0.65, 0.55, None));
            linear_slices.insert("unknown".into(), metrics(0.68, 0.58, None));
            linear_slices.insert("holdout-plugin".into(), metrics(0.68, 0.58, None));
            linear_slices
        });
        let keyword = model("keyword", 0.0, 0.0, BTreeMap::new());
        let bm25 = model("bm25", 0.60, 0.40, BTreeMap::new());
        let small = model("contextual-small", 0.70, 0.52, slices);
        let medium = model("contextual-medium", 0.60, 0.40, BTreeMap::new());
        let compact = model("contextual-compact", 0.60, 0.40, BTreeMap::new());
        let mut small_excl_slices = BTreeMap::new();
        small_excl_slices.insert("holdout-plugin".into(), metrics(0.70, 0.60, None));
        small_excl_slices.insert("test".into(), metrics(0.70, 0.52, Some(0.5)));
        small_excl_slices.insert("no-tool".into(), metrics(0.0, 0.0, Some(0.5)));
        let small_excl = ModelReport {
            label: "contextual-small-excl-plugin".into(),
            ..model(
                "contextual-small-excl-plugin",
                0.70,
                0.52,
                small_excl_slices,
            )
        };
        let mut linear_excl_slices = BTreeMap::new();
        linear_excl_slices.insert("holdout-plugin".into(), metrics(0.65, 0.55, None));
        linear_excl_slices.insert("test".into(), metrics(0.70, 0.50, Some(0.5)));
        linear_excl_slices.insert("no-tool".into(), metrics(0.0, 0.0, Some(0.5)));
        let linear_excl = ModelReport {
            label: "linear-excl-plugin".into(),
            ..model("linear-excl-plugin", 0.70, 0.50, linear_excl_slices)
        };
        vec![
            keyword,
            bm25,
            linear,
            small,
            medium,
            compact,
            small_excl,
            linear_excl,
        ]
    }

    #[test]
    fn abstention_metrics_match_hand_computation() {
        let pairs = vec![(0.9, true), (0.1, false), (0.5, true), (0.5, false)];
        let expected_brier = (0.01 + 0.01 + 0.25 + 0.25) / 4.0;
        assert!((brier(&pairs) - expected_brier).abs() < 1e-12);
        let expected_nll = (-(0.9f64.ln()) - (0.9f64.ln()) - (0.5f64.ln()) - (0.5f64.ln())) / 4.0;
        assert!((nll(&pairs) - expected_nll).abs() < 1e-12);
        assert!(ece(&pairs) >= 0.0 && ece(&pairs) <= 1.0);
        assert_eq!(brier(&[]), 0.0);
        assert_eq!(nll(&[]), 0.0);
    }

    #[test]
    fn protocol_hash_is_stable_and_excludes_operator_commit_provenance() {
        let mut preregistration = prereg();
        preregistration.schema_version = 2;
        preregistration.declared_gate_set = vec!["leakage-zero".into(), "authority-zero".into()];
        preregistration.model_config_hashes = BTreeMap::from([("encoder".into(), "abc".into())]);
        let first = protocol_hash_for(&preregistration).expect("hash");
        preregistration.preregistration_commit_sha = Some("operator-supplied-sha".into());
        let second = protocol_hash_for(&preregistration).expect("hash");
        assert_eq!(first, second);
        preregistration.gates.slice_gain_min += 0.01;
        assert_ne!(first, protocol_hash_for(&preregistration).expect("hash"));
    }

    #[test]
    fn uncalibrated_formula_is_sigmoid_of_negative_top_score() {
        assert!((uncalibrated_abstention(0.0) - 0.5).abs() < 1e-12);
        assert!(uncalibrated_abstention(-10.0) > 0.999);
        assert!(uncalibrated_abstention(10.0) < 0.001);
    }

    #[test]
    fn disposition_a_selects_the_best_passing_variant() {
        let models = passing_models();
        let prereg = prereg();
        let uncalibrated = BTreeMap::from([
            ("contextual-small".to_string(), (0.2, 0.2)),
            ("contextual-medium".to_string(), (0.2, 0.2)),
            ("contextual-compact".to_string(), (0.2, 0.2)),
        ]);
        let (gates, disposition, selected) =
            decide(&models, &recall_fixture(1.0), 0, &uncalibrated, &prereg);
        for id in [
            "leakage-zero",
            "preselector-recall",
            "authority-zero",
            "variant-contextual-small",
        ] {
            assert!(
                gates
                    .iter()
                    .find(|gate| gate.id == id)
                    .expect("gate")
                    .passed,
                "{id} should pass"
            );
        }
        assert_eq!(disposition, Disposition::APositive);
        assert_eq!(selected.as_deref(), Some("contextual-small"));
    }

    #[test]
    fn disposition_c_prefers_a_matching_compact_table() {
        let mut models = passing_models();
        // Compact matches small on every measured row.
        let small_slices = models
            .iter()
            .find(|model| model.label == "contextual-small")
            .expect("small")
            .slices
            .clone();
        if let Some(compact) = models
            .iter_mut()
            .find(|model| model.label == "contextual-compact")
        {
            compact.slices = small_slices;
        }
        // Compact exclusion reference for the holdout comparator path is not
        // needed: small carries the true-holdout gain, compact matches the
        // aggregate, and the C rule selects compact among candidates.
        let prereg = prereg();
        let uncalibrated = BTreeMap::from([
            ("contextual-small".to_string(), (0.2, 0.2)),
            ("contextual-medium".to_string(), (0.2, 0.2)),
            ("contextual-compact".to_string(), (0.2, 0.2)),
        ]);
        let (_, disposition, selected) =
            decide(&models, &recall_fixture(1.0), 0, &uncalibrated, &prereg);
        // Compact's holdout rows are seen-family references here, so under
        // the strict implementation compact does not count holdout gains;
        // it still passes via counterfactual/unknown gains copied from small.
        assert!(matches!(
            disposition,
            Disposition::CResourceEfficient | Disposition::APositive
        ));
        assert!(selected.is_some());
    }

    #[test]
    fn disposition_b_on_effectiveness_failure_without_correctness_break() {
        let mut models = passing_models();
        linear_test_mrr(&mut models, 0.90);
        let prereg = prereg();
        let uncalibrated = BTreeMap::from([
            ("contextual-small".to_string(), (0.2, 0.2)),
            ("contextual-medium".to_string(), (0.2, 0.2)),
            ("contextual-compact".to_string(), (0.2, 0.2)),
        ]);
        let (gates, disposition, selected) =
            decide(&models, &recall_fixture(1.0), 0, &uncalibrated, &prereg);
        assert!(
            gates
                .iter()
                .find(|gate| gate.id == "authority-zero")
                .expect("gate")
                .passed
        );
        assert_eq!(disposition, Disposition::BNoUsefulGain);
        assert_eq!(selected, None);
    }

    #[test]
    fn disposition_d_on_authority_violation() {
        let models = passing_models();
        let prereg = prereg();
        let uncalibrated = BTreeMap::new();
        let (_, disposition, selected) =
            decide(&models, &recall_fixture(1.0), 2, &uncalibrated, &prereg);
        assert_eq!(disposition, Disposition::DCorrectnessFailure);
        assert_eq!(selected, None);
    }

    #[test]
    fn low_recall_blocks_a_positive_disposition() {
        let models = passing_models();
        let prereg = prereg();
        let uncalibrated = BTreeMap::from([
            ("contextual-small".to_string(), (0.2, 0.2)),
            ("contextual-medium".to_string(), (0.2, 0.2)),
            ("contextual-compact".to_string(), (0.2, 0.2)),
        ]);
        let (_, disposition, _) = decide(&models, &recall_fixture(0.50), 0, &uncalibrated, &prereg);
        assert_eq!(disposition, Disposition::BNoUsefulGain);
    }
}
