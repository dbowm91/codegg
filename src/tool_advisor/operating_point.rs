//! M004 retrieval and promotion operating-point selection.
//!
//! Fixes the two v3 operating-point failures on train/dev-only
//! evidence: K=16 retrieval recall below the large-catalog gates, and
//! promotion reusing the abstention threshold as a candidate-score
//! threshold. All selection happens on clean train/dev fixtures; v3 is
//! a post-freeze diagnostic only.

use super::sequence_ranking::{
    calibrated_relevance_probability, load_artifact as load_ranker, SequenceRanker,
};
use super::sequence_retrieval::{
    frontier as retrieval_frontier, parse_mode, validate_expanded_fixture, HybridRetriever,
    RetrievalFrontierPoint, RetrievalMode,
};
use super::{
    dataset_fingerprint, load_cases, parse_jsonl, partition_cases, ToolAdvisorCandidate,
    ToolAdvisorCase,
};
use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

pub const OPERATING_POINT_SCHEMA_VERSION: u16 = 1;
pub const OPERATING_POINT_PROTOCOL: &str = "m004-preregistered-operating-point-v1";

/// Predeclared retrieval frontier: universes × shortlists × modes.
pub const FRONTIER_UNIVERSES: [usize; 3] = [64, 128, 256];
pub const FRONTIER_KS: [usize; 3] = [16, 24, 32];

/// Predeclared promotion threshold grid (5-point resolution).
pub fn promotion_threshold_grid() -> Vec<f64> {
    (1..=19).map(|step| step as f64 * 0.05).collect()
}

pub const PROMOTION_MAX_PROMOTIONS: usize = 2;
pub const PROMOTION_SCHEMA_BUDGET_BYTES: usize = 16 * 1024;

/// Expand dev/train cases to exactly `size` deferred candidates.
///
/// Every labeled relevant tool is preserved (re-homed into the
/// deferred set when necessary); the remainder is deterministic
/// synthetic filler disjoint from every relevant name. The output is
/// name-sorted, validated, and carries the source labels, groups, and
/// split ownership unchanged.
pub fn expand_universe(cases: &[ToolAdvisorCase], size: usize) -> Result<Vec<ToolAdvisorCase>> {
    if size == 0 {
        return Err(anyhow!("retrieval universe size must be positive"));
    }
    let reserved: BTreeSet<String> = cases
        .iter()
        .flat_map(|case| case.relevance.keys().cloned())
        .chain(cases.iter().flat_map(|case| {
            case.candidates
                .iter()
                .map(|candidate| candidate.name.clone())
        }))
        .collect();
    let mut filler = Vec::new();
    let mut index = 0usize;
    while filler.len() < size {
        let name = format!("fixture_deferred_{index:03}");
        if !reserved.contains(&name) {
            filler.push(ToolAdvisorCandidate {
                name: name.clone(),
                description: format!("Deterministic M004 expanded fixture descriptor {index}"),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: true,
            });
        }
        index += 1;
    }
    let mut expanded = Vec::with_capacity(cases.len());
    for case in cases {
        let mut fixture = case.clone();
        let mut universe: Vec<ToolAdvisorCandidate> = case
            .candidates
            .iter()
            .filter(|candidate| candidate.disclosure == "deferred")
            .cloned()
            .collect();
        for name in case.relevance.keys() {
            if !universe.iter().any(|candidate| &candidate.name == name) {
                match case
                    .candidates
                    .iter()
                    .find(|candidate| &candidate.name == name)
                {
                    Some(candidate) => {
                        let mut preserved = candidate.clone();
                        preserved.disclosure = "deferred".into();
                        universe.push(preserved);
                    }
                    None => {
                        universe.push(ToolAdvisorCandidate {
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
        }
        universe.sort_by(|left, right| left.name.cmp(&right.name));
        universe.truncate(size);
        let mut seen: BTreeSet<String> = universe
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect();
        for candidate in &filler {
            if universe.len() >= size {
                break;
            }
            if seen.insert(candidate.name.clone()) {
                universe.push(candidate.clone());
            }
        }
        universe.sort_by(|left, right| left.name.cmp(&right.name));
        fixture.candidates = universe;
        expanded.push(fixture);
    }
    validate_expanded_fixture(&expanded, size, FRONTIER_KS[0].min(size))?;
    Ok(expanded)
}

fn deferred_names(case: &ToolAdvisorCase) -> BTreeSet<String> {
    case.candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.clone())
        .collect()
}

/// Retrieval recall of one frontier point, re-derived from its counts.
pub fn point_recall(point: &RetrievalFrontierPoint) -> f64 {
    if point.eligible_relevant_tools == 0 {
        1.0
    } else {
        point.recovered_relevant_tools as f64 / point.eligible_relevant_tools as f64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSelection {
    pub mode: String,
    pub k: usize,
    pub recall_64: f64,
    pub recall_128: f64,
    pub recall_256: f64,
    pub mean_latency_ms: f64,
    pub max_latency_ms: u128,
    pub authority_violations: usize,
}

/// Apply the M004 retrieval gates and pick the smallest/cheapest
/// operating point.
///
/// Gates: 64-recall ≥ 0.99, 128-recall ≥ 0.98, 256-recall ≥ 0.95,
/// zero authority violations, K ≤ 32. Selection minimizes K first,
/// then measured mean latency, then mode name (deterministic).
/// No point clearing the frontier returns an error: the caller closes
/// negatively instead of silently raising the candidate budget.
pub fn select_retrieval_operating_point(
    points_64: &[RetrievalFrontierPoint],
    points_128: &[RetrievalFrontierPoint],
    points_256: &[RetrievalFrontierPoint],
    authority_violations: usize,
) -> Result<RetrievalSelection> {
    if authority_violations > 0 {
        return Err(anyhow!(
            "retrieval frontier has {authority_violations} authority violations"
        ));
    }
    let recall_of =
        |points: &[RetrievalFrontierPoint], mode: &str, k: usize| -> Option<(f64, f64, u128)> {
            points
                .iter()
                .find(|point| point.mode == mode && point.shortlist_k == k)
                .map(|point| {
                    (
                        point_recall(point),
                        point.mean_latency_ms,
                        point.max_latency_ms,
                    )
                })
        };
    let mut feasible = Vec::new();
    for mode in ["bm25", "semantic", "rrf", "normalized-union"] {
        for k in FRONTIER_KS {
            let Some((r64, _, _)) = recall_of(points_64, mode, k) else {
                continue;
            };
            let Some((r128, _, _)) = recall_of(points_128, mode, k) else {
                continue;
            };
            let Some((r256, mean_latency_ms, max_latency_ms)) = recall_of(points_256, mode, k)
            else {
                continue;
            };
            if r64 >= 0.99 && r128 >= 0.98 && r256 >= 0.95 {
                feasible.push(RetrievalSelection {
                    mode: mode.into(),
                    k,
                    recall_64: r64,
                    recall_128: r128,
                    recall_256: r256,
                    mean_latency_ms,
                    max_latency_ms,
                    authority_violations: 0,
                });
            }
        }
    }
    feasible.sort_by(|left, right| {
        left.k
            .cmp(&right.k)
            .then_with(|| {
                left.mean_latency_ms
                    .partial_cmp(&right.mean_latency_ms)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.mode.cmp(&right.mode))
    });
    feasible
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no retrieval operating point clears 0.99/0.98/0.95 at K<=32"))
}

// ---- promotion ----

/// Per-case promotion inputs: calibrated relevance per candidate plus
/// the separate abstention probability.
#[derive(Debug, Clone)]
pub struct PromotionCaseView {
    pub case_id: String,
    pub none: bool,
    pub relevant: BTreeSet<String>,
    pub abstained: bool,
    pub relevance: BTreeMap<String, f64>,
    pub allowed_deferred: BTreeSet<String>,
    pub schema_bytes_per_candidate: BTreeMap<String, usize>,
}

fn promotion_schema_bytes(candidate: &ToolAdvisorCandidate) -> usize {
    serde_json::to_vec(candidate)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

/// Score every dev case once; thresholds sweep over these views.
pub fn promotion_case_views(
    ranker: &SequenceRanker,
    cases: &[ToolAdvisorCase],
    abstention_threshold: f64,
    temperature: f64,
    bias: f64,
) -> Result<Vec<PromotionCaseView>> {
    let mut views = Vec::with_capacity(cases.len());
    for case in cases {
        let (prediction, _, _) = ranker.predict_case(case)?;
        let abstained = prediction.abstain_probability.unwrap_or(1.0) >= abstention_threshold;
        let by_name: BTreeMap<&str, f64> = prediction
            .ranked
            .iter()
            .map(|item| (item.name.as_str(), item.score))
            .collect();
        let mut relevance = BTreeMap::new();
        let mut schema_bytes = BTreeMap::new();
        for candidate in &case.candidates {
            if let Some(logit) = by_name.get(candidate.name.as_str()).copied() {
                relevance.insert(
                    candidate.name.clone(),
                    calibrated_relevance_probability(logit, temperature, bias),
                );
                schema_bytes.insert(candidate.name.clone(), promotion_schema_bytes(candidate));
            }
        }
        views.push(PromotionCaseView {
            case_id: case.case_id.clone(),
            none: case.none,
            relevant: case.relevance.keys().cloned().collect(),
            abstained,
            relevance,
            allowed_deferred: deferred_names(case),
            schema_bytes_per_candidate: schema_bytes,
        });
    }
    Ok(views)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionOutcome {
    pub threshold: f64,
    pub relevant_recall: f64,
    pub relevant_total: usize,
    pub relevant_promoted: usize,
    pub no_tool_promotion_rate: f64,
    pub no_tool_cases: usize,
    pub irrelevant_promotion_rate: f64,
    pub promoted_total: usize,
    pub max_promotions_observed: usize,
    pub schema_p95_bytes: usize,
    pub meets_constraints: bool,
}

/// Evaluate one promotion threshold over scored case views.
///
/// Eligibility per candidate: advisor not abstaining, calibrated
/// relevance ≥ threshold, member of the authority-filtered deferred
/// set, at most `PROMOTION_MAX_PROMOTIONS` per case (top confidence
/// first). Rates are micro-averaged; cases with no promotions
/// contribute an irrelevant rate of zero.
pub fn promotion_outcome_for_threshold(
    views: &[PromotionCaseView],
    threshold: f64,
) -> PromotionOutcome {
    let mut relevant_total = 0usize;
    let mut relevant_promoted = 0usize;
    let mut no_tool_cases = 0usize;
    let mut no_tool_promoted = 0usize;
    let mut promoted_total = 0usize;
    let mut irrelevant_promoted = 0usize;
    let mut max_promotions_observed = 0usize;
    let mut schema_bytes = Vec::new();
    for view in views {
        let mut eligible: Vec<(&String, f64)> = view
            .relevance
            .iter()
            .filter(|(name, _)| view.allowed_deferred.contains(*name))
            .filter(|(_, prob)| **prob >= threshold)
            .map(|(name, prob)| (name, *prob))
            .collect();
        // Abstention suppresses every promotion for the case.
        if view.abstained {
            eligible.clear();
        }
        eligible.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(right.0))
        });
        eligible.truncate(PROMOTION_MAX_PROMOTIONS);
        max_promotions_observed = max_promotions_observed.max(eligible.len());
        let promoted: BTreeSet<&String> = eligible.iter().map(|(name, _)| *name).collect();
        schema_bytes.push(
            promoted
                .iter()
                .map(|name| {
                    view.schema_bytes_per_candidate
                        .get(*name)
                        .copied()
                        .unwrap_or(0)
                })
                .sum::<usize>(),
        );
        if view.none {
            no_tool_cases += 1;
            if !promoted.is_empty() {
                no_tool_promoted += 1;
            }
            irrelevant_promoted += promoted.len();
            promoted_total += promoted.len();
        } else {
            relevant_total += view.relevant.len();
            for name in &promoted {
                if view.relevant.contains(*name) {
                    relevant_promoted += 1;
                } else {
                    irrelevant_promoted += 1;
                }
            }
            promoted_total += promoted.len();
        }
    }
    schema_bytes.sort_unstable();
    let schema_p95_bytes = if schema_bytes.is_empty() {
        0
    } else {
        schema_bytes[(0.95 * (schema_bytes.len() as f64 - 1.0)).round() as usize]
    };
    let relevant_recall = if relevant_total == 0 {
        0.0
    } else {
        relevant_promoted as f64 / relevant_total as f64
    };
    let no_tool_promotion_rate = if no_tool_cases == 0 {
        0.0
    } else {
        no_tool_promoted as f64 / no_tool_cases as f64
    };
    let irrelevant_promotion_rate = if promoted_total == 0 {
        0.0
    } else {
        irrelevant_promoted as f64 / promoted_total as f64
    };
    PromotionOutcome {
        threshold,
        relevant_recall,
        relevant_total,
        relevant_promoted,
        no_tool_promotion_rate,
        no_tool_cases,
        irrelevant_promotion_rate,
        promoted_total,
        max_promotions_observed,
        schema_p95_bytes,
        meets_constraints: no_tool_promotion_rate <= 0.10
            && irrelevant_promotion_rate <= 0.15
            && max_promotions_observed <= PROMOTION_MAX_PROMOTIONS
            && schema_p95_bytes <= PROMOTION_SCHEMA_BUDGET_BYTES,
    }
}

/// Select the highest-recall threshold meeting every constraint.
/// Ties resolve to the lower schema cost, then the higher threshold
/// (more conservative), deterministically.
pub fn select_promotion_threshold(outcomes: &[PromotionOutcome]) -> Result<PromotionOutcome> {
    let mut feasible: Vec<&PromotionOutcome> = outcomes
        .iter()
        .filter(|outcome| outcome.meets_constraints)
        .collect();
    feasible.sort_by(|left, right| {
        right
            .relevant_recall
            .partial_cmp(&left.relevant_recall)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.schema_p95_bytes.cmp(&right.schema_p95_bytes))
            .then_with(|| {
                right
                    .threshold
                    .partial_cmp(&left.threshold)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    feasible.into_iter().next().cloned().ok_or_else(|| {
        anyhow!("no promotion threshold meets recall/FPR/no-tool/schema constraints")
    })
}

// ---- operating-point artifact ----

/// Frozen M004 operating point. Abstention calibration, candidate
/// relevance calibration, promotion threshold, and retrieval mode/K
/// are separate fields; constructors reject any conflation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisorOperatingPoint {
    pub schema_version: u16,
    pub protocol: String,
    pub model_artifact: String,
    pub model_artifact_sha256: String,
    pub model_architecture: String,
    pub abstention_threshold: f64,
    pub candidate_temperature: f64,
    pub candidate_bias: f64,
    pub promotion_threshold: f64,
    pub retrieval_mode: String,
    pub retrieval_k: usize,
    pub max_promotions: usize,
    pub schema_budget_bytes: usize,
    pub permutation_contract_version: u16,
    pub training_objective_version: String,
    pub dev_fingerprint: String,
}

impl AdvisorOperatingPoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model_artifact: &str,
        model_artifact_sha256: &str,
        model_architecture: &str,
        abstention_threshold: f64,
        candidate_temperature: f64,
        candidate_bias: f64,
        promotion_threshold: f64,
        retrieval_mode: &str,
        retrieval_k: usize,
        dev_fingerprint: &str,
    ) -> Result<Self> {
        if !promotion_threshold.is_finite() || !(0.0..=1.0).contains(&promotion_threshold) {
            return Err(anyhow!("promotion threshold must be a probability"));
        }
        if (promotion_threshold - abstention_threshold).abs() < 1e-9 {
            return Err(anyhow!(
                "promotion threshold must not reuse the abstention threshold value"
            ));
        }
        if retrieval_k == 0 || retrieval_k > 32 {
            return Err(anyhow!("retrieval K must be within 1..=32"));
        }
        parse_mode(retrieval_mode)?;
        Ok(Self {
            schema_version: OPERATING_POINT_SCHEMA_VERSION,
            protocol: OPERATING_POINT_PROTOCOL.into(),
            model_artifact: model_artifact.into(),
            model_artifact_sha256: model_artifact_sha256.into(),
            model_architecture: model_architecture.into(),
            abstention_threshold,
            candidate_temperature,
            candidate_bias,
            promotion_threshold,
            retrieval_mode: retrieval_mode.into(),
            retrieval_k,
            max_promotions: PROMOTION_MAX_PROMOTIONS,
            schema_budget_bytes: PROMOTION_SCHEMA_BUDGET_BYTES,
            permutation_contract_version: super::order_invariance::PERMUTATION_CONTRACT_VERSION,
            training_objective_version: super::sequence_ranking::TRAINING_OBJECTIVE_GRADED_V1
                .into(),
            dev_fingerprint: dev_fingerprint.into(),
        })
    }
}

// ---- selection driver ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatingPointSweepConfig {
    pub schema_version: u16,
    pub protocol: String,
    pub ranker_artifact: String,
    pub encoder_manifest: String,
    pub dataset: String,
    pub output_dir: String,
    pub seed: u64,
    pub promotion_sample_cases: usize,
    pub promotion_permutations_per_case: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionOrderEvidence {
    pub sampled_cases: usize,
    pub permutations_per_case: usize,
    pub identity_consistency: f64,
    pub threshold_crossing_cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEvidence {
    pub retrieval_p50_ms: f64,
    pub retrieval_p95_ms: f64,
    pub retrieval_max_ms: u128,
    pub ranking_p50_ms: f64,
    pub ranking_p95_ms: f64,
    pub ranking_max_ms: u128,
    pub ranker_forwards_per_case: usize,
    pub cache_entries: usize,
    pub schema_p95_bytes: usize,
    pub k_scaling: Vec<(usize, f64, u128)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OperatingDiagnostic {
    pub retrieval_recall_64: f64,
    pub retrieval_recall_128: f64,
    pub promotion_recall: f64,
    pub promotion_no_tool_rate: f64,
    pub promotion_irrelevant_rate: f64,
    pub non_gating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M004OperatingPointReport {
    pub schema_version: u16,
    pub protocol: String,
    pub sweep_fingerprint: String,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub retrieval: RetrievalSelection,
    pub promotion_grid: Vec<PromotionOutcome>,
    pub promotion: PromotionOutcome,
    pub promotion_order: PromotionOrderEvidence,
    pub resources: ResourceEvidence,
    pub operating_point: AdvisorOperatingPoint,
    pub train_confirmation_recall: f64,
    pub v3_diagnostic: V3OperatingDiagnostic,
}

fn sweep_fingerprint(config: &OperatingPointSweepConfig) -> Result<String> {
    Ok(hex::encode(Sha256::digest(
        serde_json::to_string(config)?.as_bytes(),
    )))
}

fn artifact_file_sha256(path: &str) -> Result<String> {
    Ok(hex::encode(Sha256::digest(
        std::fs::read(path).with_context(|| format!("read artifact {path}"))?,
    )))
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[(quantile * (sorted.len() as f64 - 1.0)).round() as usize % sorted.len()]
}

/// Promotion identity under candidate permutation: the promoted set
/// must not change with presentation order, and no candidate may
/// cross the frozen threshold from position alone.
#[allow(clippy::too_many_arguments)]
pub fn promotion_order_evidence(
    ranker: &SequenceRanker,
    cases: &[ToolAdvisorCase],
    sample_cases: usize,
    permutations_per_case: usize,
    seed: u64,
    abstention_threshold: f64,
    temperature: f64,
    bias: f64,
    promotion_threshold: f64,
) -> Result<PromotionOrderEvidence> {
    use super::order_invariance::{generate_permutations, permute_case};
    let mut ordered = cases.to_vec();
    ordered.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    let mut agreements = 0usize;
    let mut comparisons = 0usize;
    let mut crossing_cases = 0usize;
    for case in ordered.iter().take(sample_cases.max(1)) {
        let canonical = promotion_case_views(
            ranker,
            std::slice::from_ref(case),
            abstention_threshold,
            temperature,
            bias,
        )?
        .pop()
        .ok_or_else(|| anyhow!("missing canonical promotion view"))?;
        let canonical_set = promoted_set(&canonical, promotion_threshold);
        let mut case_crossed = false;
        for permutation in
            generate_permutations(case.candidates.len(), seed, permutations_per_case.max(1))
                .iter()
                .skip(1)
        {
            let permuted = permute_case(case, permutation)?;
            let view = promotion_case_views(
                ranker,
                std::slice::from_ref(&permuted),
                abstention_threshold,
                temperature,
                bias,
            )?
            .pop()
            .ok_or_else(|| anyhow!("missing permuted promotion view"))?;
            let permuted_set = promoted_set(&view, promotion_threshold);
            comparisons += 1;
            if permuted_set == canonical_set {
                agreements += 1;
            } else {
                case_crossed = true;
            }
            // Threshold crossing from position alone: any candidate
            // whose eligibility differs between presentations.
            for name in view.relevance.keys() {
                let canonical_eligible = canonical
                    .relevance
                    .get(name)
                    .is_some_and(|prob| *prob >= promotion_threshold)
                    && canonical.allowed_deferred.contains(name)
                    && !canonical.abstained;
                let permuted_eligible = view
                    .relevance
                    .get(name)
                    .is_some_and(|prob| *prob >= promotion_threshold)
                    && view.allowed_deferred.contains(name)
                    && !view.abstained;
                if canonical_eligible != permuted_eligible {
                    case_crossed = true;
                }
            }
        }
        if case_crossed {
            crossing_cases += 1;
        }
    }
    Ok(PromotionOrderEvidence {
        sampled_cases: sample_cases.min(cases.len()),
        permutations_per_case,
        identity_consistency: if comparisons == 0 {
            1.0
        } else {
            agreements as f64 / comparisons as f64
        },
        threshold_crossing_cases: crossing_cases,
    })
}

fn promoted_set(view: &PromotionCaseView, threshold: f64) -> BTreeSet<String> {
    let mut eligible: Vec<(&String, f64)> = view
        .relevance
        .iter()
        .filter(|(name, _)| view.allowed_deferred.contains(*name))
        .filter(|(_, prob)| **prob >= threshold)
        .map(|(name, prob)| (name, *prob))
        .collect();
    if view.abstained {
        eligible.clear();
    }
    eligible.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    eligible.truncate(PROMOTION_MAX_PROMOTIONS);
    eligible.into_iter().map(|(name, _)| name.clone()).collect()
}

/// Run the predeclared M004 operating-point selection.
pub fn run_operating_point_selection(
    sweep: &OperatingPointSweepConfig,
    device: &Device,
) -> Result<M004OperatingPointReport> {
    if sweep.schema_version != OPERATING_POINT_SCHEMA_VERSION {
        return Err(anyhow!("unsupported operating-point sweep schema"));
    }
    let fingerprint = sweep_fingerprint(sweep)?;
    let cases = load_cases(Some(std::path::Path::new(&sweep.dataset)))?;
    let dataset_fp = dataset_fingerprint(&cases)?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let train_cases: Vec<ToolAdvisorCase> = partition
        .train_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases)?;
    let ranker = load_ranker(std::path::Path::new(&sweep.ranker_artifact), device)?;
    let abstention_threshold = ranker.manifest.calibration.abstention_threshold;
    let temperature = ranker.manifest.calibration.candidate_temperature;
    let bias = ranker.manifest.calibration.candidate_bias;
    let encoder = super::sequence_encoder::CandleBertSequenceEncoder::load(
        std::path::Path::new(&sweep.encoder_manifest),
        device,
    )?;
    std::fs::create_dir_all(&sweep.output_dir)?;

    // Retrieval frontier on dev universes (selection) + train
    // confirmation at the selected point only. Each universe frontier
    // checkpoints to disk so an interrupted run resumes without
    // recomputing finished universes; checkpoints bind the sweep
    // fingerprint and are ignored on any mismatch.
    #[derive(Serialize, Deserialize)]
    struct FrontierCheckpoint {
        sweep_fingerprint: String,
        universe_size: usize,
        points: Vec<RetrievalFrontierPoint>,
        authority_violations: usize,
    }
    let checkpoint_path = |size: usize| {
        std::path::Path::new(&sweep.output_dir)
            .join(format!("m004-checkpoint-frontier-{size}.json"))
    };
    let mut retriever = HybridRetriever::new(&encoder);
    let surface = format!("m004-dev-universes:{dataset_fp}");
    let modes = [
        RetrievalMode::Bm25,
        RetrievalMode::Semantic,
        RetrievalMode::Rrf,
        RetrievalMode::NormalizedUnion,
    ];
    let mut frontier_by_universe: BTreeMap<usize, Vec<RetrievalFrontierPoint>> = BTreeMap::new();
    let mut violations = 0usize;
    for size in FRONTIER_UNIVERSES {
        if let Ok(bytes) = std::fs::read(checkpoint_path(size)) {
            if let Ok(checkpoint) = serde_json::from_slice::<FrontierCheckpoint>(&bytes) {
                if checkpoint.sweep_fingerprint == fingerprint && checkpoint.universe_size == size {
                    frontier_by_universe.insert(size, checkpoint.points);
                    violations += checkpoint.authority_violations;
                    continue;
                }
            }
        }
        let fixture = expand_universe(&dev_cases, size)?;
        let report = retrieval_frontier(
            &mut retriever,
            &fixture,
            &format!("{surface}:{size}"),
            &FRONTIER_KS,
            &modes,
        )?;
        // Authority check: the frontier only scores eligible deferred
        // descriptors, so any recovered name outside the case universe
        // is a violation. Re-resolve every point explicitly.
        let mut universe_violations = 0usize;
        for case in &fixture {
            let allowed = deferred_names(case);
            for mode in &modes {
                for k in FRONTIER_KS {
                    let result = retriever.retrieve_with_fallback(case, &surface, *mode, k);
                    for name in &result.names {
                        if !allowed.contains(name) {
                            universe_violations += 1;
                        }
                    }
                }
            }
        }
        violations += universe_violations;
        frontier_by_universe.insert(size, report.points.clone());
        std::fs::write(
            checkpoint_path(size),
            serde_json::to_vec_pretty(&FrontierCheckpoint {
                sweep_fingerprint: fingerprint.clone(),
                universe_size: size,
                points: report.points,
                authority_violations: universe_violations,
            })?,
        )?;
    }
    let selection = select_retrieval_operating_point(
        &frontier_by_universe[&64],
        &frontier_by_universe[&128],
        &frontier_by_universe[&256],
        violations,
    )?;
    let selected_mode = parse_mode(&selection.mode)?;

    // Train confirmation at the selected point (no selection from train).
    let train_fixture = expand_universe(&train_cases, 128)?;
    let train_report = retrieval_frontier(
        &mut retriever,
        &train_fixture,
        &format!("m004-train-confirmation:{dataset_fp}"),
        &[selection.k],
        &[selected_mode],
    )?;
    let train_confirmation_recall = train_report.points.first().map(point_recall).unwrap_or(0.0);

    // Promotion threshold sweep on dev (predeclared grid).
    let views = promotion_case_views(&ranker, &dev_cases, abstention_threshold, temperature, bias)?;
    let grid = promotion_threshold_grid();
    let outcomes: Vec<PromotionOutcome> = grid
        .iter()
        .map(|threshold| promotion_outcome_for_threshold(&views, *threshold))
        .collect();
    let promotion = select_promotion_threshold(&outcomes)?;
    if promotion.relevant_recall < 0.60 {
        return Err(anyhow!(
            "promotion recall {:.3} misses the 0.60 dev target at every feasible threshold",
            promotion.relevant_recall
        ));
    }

    // Promotion order robustness on a deterministic dev sample.
    let order = promotion_order_evidence(
        &ranker,
        &dev_cases,
        sweep.promotion_sample_cases,
        sweep.promotion_permutations_per_case,
        sweep.seed,
        abstention_threshold,
        temperature,
        bias,
        promotion.threshold,
    )?;
    if order.identity_consistency < 0.95 || order.threshold_crossing_cases > 0 {
        return Err(anyhow!(
            "promotion order gate fails: consistency {:.3}, crossing cases {}",
            order.identity_consistency,
            order.threshold_crossing_cases
        ));
    }

    // Resources at the selected point.
    let mut retrieval_latencies = Vec::new();
    let dev_128 = expand_universe(&dev_cases, 128)?;
    for case in &dev_128 {
        let started = Instant::now();
        let _ = retriever.retrieve(case, &surface, selected_mode, selection.k)?;
        retrieval_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    retrieval_latencies
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranking_latencies = Vec::new();
    let mut forwards = 0usize;
    for case in &dev_cases {
        let started = Instant::now();
        let (_, _, case_forwards) = ranker.predict_case(case)?;
        ranking_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
        forwards = forwards.max(case_forwards);
    }
    ranking_latencies
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mut k_scaling = Vec::new();
    for k in FRONTIER_KS {
        let started = Instant::now();
        for case in &dev_128 {
            let _ = retriever.retrieve(case, &surface, selected_mode, k)?;
        }
        let elapsed = started.elapsed();
        k_scaling.push((
            k,
            elapsed.as_secs_f64() * 1000.0 / dev_128.len() as f64,
            elapsed.as_millis(),
        ));
    }
    let resources = ResourceEvidence {
        retrieval_p50_ms: percentile(&retrieval_latencies, 0.50),
        retrieval_p95_ms: percentile(&retrieval_latencies, 0.95),
        retrieval_max_ms: retrieval_latencies.last().copied().unwrap_or(0.0) as u128,
        ranking_p50_ms: percentile(&ranking_latencies, 0.50),
        ranking_p95_ms: percentile(&ranking_latencies, 0.95),
        ranking_max_ms: ranking_latencies.last().copied().unwrap_or(0.0) as u128,
        ranker_forwards_per_case: forwards,
        cache_entries: retriever.cache.len(),
        schema_p95_bytes: promotion.schema_p95_bytes,
        k_scaling,
    };

    // Freeze the operating point (separation enforced by constructor).
    let operating_point = AdvisorOperatingPoint::new(
        &sweep.ranker_artifact,
        &artifact_file_sha256(&sweep.ranker_artifact)?,
        &ranker.manifest.architecture,
        abstention_threshold,
        temperature,
        bias,
        promotion.threshold,
        &selection.mode,
        selection.k,
        &dev_fp,
    )?;

    // Post-freeze v3 diagnostic only (no feedback).
    let v3_cases = parse_jsonl(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/tool-advisor/qualification-v3-holdout.jsonl"),
        )
        .context("read v3 holdout")?,
    )?;
    let v3_64 = expand_universe(&v3_cases, 64)?;
    let v3_128 = expand_universe(&v3_cases, 128)?;
    let v3_frontier_64 = retrieval_frontier(
        &mut retriever,
        &v3_64,
        "m004-v3-diagnostic:64",
        &[selection.k],
        &[selected_mode],
    )?;
    let v3_frontier_128 = retrieval_frontier(
        &mut retriever,
        &v3_128,
        "m004-v3-diagnostic:128",
        &[selection.k],
        &[selected_mode],
    )?;
    let v3_views =
        promotion_case_views(&ranker, &v3_cases, abstention_threshold, temperature, bias)?;
    let v3_promotion = promotion_outcome_for_threshold(&v3_views, promotion.threshold);
    let v3_diagnostic = V3OperatingDiagnostic {
        retrieval_recall_64: v3_frontier_64.points.first().map_or(0.0, point_recall),
        retrieval_recall_128: v3_frontier_128.points.first().map_or(0.0, point_recall),
        promotion_recall: v3_promotion.relevant_recall,
        promotion_no_tool_rate: v3_promotion.no_tool_promotion_rate,
        promotion_irrelevant_rate: v3_promotion.irrelevant_promotion_rate,
        non_gating: true,
    };

    Ok(M004OperatingPointReport {
        schema_version: OPERATING_POINT_SCHEMA_VERSION,
        protocol: sweep.protocol.clone(),
        sweep_fingerprint: fingerprint,
        dataset_fingerprint: dataset_fp,
        train_partition_fingerprint: dataset_fingerprint(&train_cases)?,
        dev_partition_fingerprint: dev_fp,
        retrieval: selection,
        promotion_grid: outcomes,
        promotion,
        promotion_order: order,
        resources,
        operating_point,
        train_confirmation_recall,
        v3_diagnostic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_view(
        case_id: &str,
        none: bool,
        relevant: &[&str],
        abstained: bool,
        probs: &[(&str, f64)],
        allowed: &[&str],
    ) -> PromotionCaseView {
        PromotionCaseView {
            case_id: case_id.into(),
            none,
            relevant: relevant.iter().map(ToString::to_string).collect(),
            abstained,
            relevance: probs
                .iter()
                .map(|(name, prob)| (name.to_string(), *prob))
                .collect(),
            allowed_deferred: allowed.iter().map(ToString::to_string).collect(),
            schema_bytes_per_candidate: probs
                .iter()
                .map(|(name, _)| (name.to_string(), 120))
                .collect(),
        }
    }

    /// M004: promotion eligibility separates abstention, relevance
    /// threshold, authority allowlist, and the per-case cap.
    #[test]
    fn promotion_threshold_outcome_separates_concerns() {
        let views = vec![
            // Relevant case: one strong + one weak relevant, one distractor.
            synthetic_view(
                "labeled",
                false,
                &["tool-a"],
                false,
                &[("tool-a", 0.9), ("tool-b", 0.2), ("tool-c", 0.8)],
                &["tool-a", "tool-b", "tool-c"],
            ),
            // Abstained case promotes nothing despite high scores.
            synthetic_view(
                "abstained",
                false,
                &["tool-a"],
                true,
                &[("tool-a", 0.95)],
                &["tool-a"],
            ),
            // No-tool case: any promotion is a false promotion.
            synthetic_view("no-tool", true, &[], false, &[("tool-x", 0.7)], &["tool-x"]),
            // Denied tool never enters the universe even above threshold.
            synthetic_view(
                "denied",
                false,
                &["tool-a"],
                false,
                &[("tool-a", 0.9), ("denied-tool", 0.99)],
                &["tool-a"],
            ),
        ];
        let outcome = promotion_outcome_for_threshold(&views, 0.5);
        // Labeled promotes tool-a + tool-c (tool-b below threshold);
        // abstained promotes nothing despite 0.95; no-tool promotes
        // tool-x (false); denied-tool is excluded by the allowlist while
        // tool-a still promotes. Relevant labels total 3 (labeled,
        // abstained, denied); 2 promote.
        assert_eq!(outcome.relevant_total, 3);
        assert_eq!(outcome.relevant_promoted, 2);
        assert!((outcome.relevant_recall - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(outcome.no_tool_cases, 1);
        assert!((outcome.no_tool_promotion_rate - 1.0).abs() < 1e-9);
        assert_eq!(outcome.promoted_total, 4);
        assert!((outcome.irrelevant_promotion_rate - 0.5).abs() < 1e-9);
        assert_eq!(outcome.max_promotions_observed, 2);
        assert!(outcome.schema_p95_bytes <= PROMOTION_SCHEMA_BUDGET_BYTES);
    }

    /// M004: threshold selection prefers recall under constraints and
    /// resolves ties deterministically.
    #[test]
    fn promotion_threshold_selection_prefers_constrained_recall() {
        let views = vec![
            synthetic_view(
                "labeled",
                false,
                &["tool-a"],
                false,
                &[("tool-a", 0.9), ("tool-b", 0.4)],
                &["tool-a", "tool-b"],
            ),
            synthetic_view("clean", true, &[], false, &[("tool-z", 0.1)], &["tool-z"]),
        ];
        let outcomes: Vec<PromotionOutcome> = promotion_threshold_grid()
            .iter()
            .map(|threshold| promotion_outcome_for_threshold(&views, *threshold))
            .collect();
        assert_eq!(outcomes.len(), 19);
        let selected = select_promotion_threshold(&outcomes).expect("selection");
        // Thresholds (0.1, 0.4] promote tool-a with zero false
        // promotions; the highest feasible recall point wins.
        assert!((selected.relevant_recall - 1.0).abs() < 1e-9);
        assert_eq!(selected.irrelevant_promotion_rate, 0.0);
        assert_eq!(selected.no_tool_promotion_rate, 0.0);
        assert!(selected.meets_constraints);
        // An impossible grid fails closed instead of relaxing.
        let impossible = vec![PromotionOutcome {
            threshold: 0.99,
            relevant_recall: 0.0,
            relevant_total: 1,
            relevant_promoted: 0,
            no_tool_promotion_rate: 0.5,
            no_tool_cases: 2,
            irrelevant_promotion_rate: 0.5,
            promoted_total: 2,
            max_promotions_observed: 1,
            schema_p95_bytes: 0,
            meets_constraints: false,
        }];
        assert!(select_promotion_threshold(&impossible).is_err());
    }

    /// M004 §9: artifact/config separation — promotion threshold can
    /// never populate the abstention field and round-trips independently.
    #[test]
    fn operating_point_fields_stay_separate() {
        let point = AdvisorOperatingPoint::new(
            "artifact.json",
            "abc123",
            "sequence-encoder-packed-shared-span-v1",
            0.627373218536377,
            1.0,
            -1.0,
            0.5,
            "rrf",
            24,
            "dev-fp",
        )
        .expect("operating point");
        assert_ne!(point.promotion_threshold, point.abstention_threshold);
        assert_eq!(point.max_promotions, PROMOTION_MAX_PROMOTIONS);
        let encoded = serde_json::to_string(&point).expect("serialize");
        let decoded: AdvisorOperatingPoint = serde_json::from_str(&encoded).expect("round-trip");
        assert_eq!(decoded.promotion_threshold, 0.5);
        assert_eq!(decoded.abstention_threshold, 0.627373218536377);
        assert_eq!(decoded.retrieval_k, 24);
        // Reusing the abstention value as promotion threshold fails closed.
        assert!(AdvisorOperatingPoint::new(
            "artifact.json",
            "abc123",
            "sequence-encoder-packed-shared-span-v1",
            0.627373218536377,
            1.0,
            -1.0,
            0.627373218536377,
            "rrf",
            24,
            "dev-fp",
        )
        .is_err());
        // Out-of-range K fails closed.
        assert!(AdvisorOperatingPoint::new(
            "artifact.json",
            "abc123",
            "sequence-encoder-packed-shared-span-v1",
            0.627373218536377,
            1.0,
            -1.0,
            0.5,
            "rrf",
            64,
            "dev-fp",
        )
        .is_err());
    }

    /// M004 §2: universe expansion preserves every relevant tool,
    /// validates, and stays deterministic.
    #[test]
    fn universe_expansion_preserves_relevant_tools() {
        let cases = load_cases(None).expect("builtin corpus");
        let dev: Vec<ToolAdvisorCase> = partition_cases(&cases)
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        for size in [64, 128] {
            let expanded = expand_universe(&dev, size).expect("expansion");
            assert_eq!(expanded.len(), dev.len());
            for (source, fixture) in dev.iter().zip(expanded.iter()) {
                assert_eq!(fixture.candidates.len(), size);
                assert_eq!(fixture.relevance, source.relevance);
                assert_eq!(fixture.case_id, source.case_id);
                for name in source.relevance.keys() {
                    assert!(
                        fixture
                            .candidates
                            .iter()
                            .any(|candidate| &candidate.name == name),
                        "relevant {name} must survive expansion"
                    );
                }
            }
            let again = expand_universe(&dev, size).expect("expansion");
            assert_eq!(expanded, again, "expansion must be deterministic");
        }
    }

    /// M004 §3: retrieval selection enforces gates literally and picks
    /// the smallest/cheapest feasible point.
    #[test]
    fn retrieval_selection_enforces_gates() {
        fn point(mode: &str, k: usize, recall: f64, mean_ms: f64) -> RetrievalFrontierPoint {
            let total = 1000;
            let recovered = (recall * total as f64) as usize;
            RetrievalFrontierPoint {
                mode: mode.into(),
                k,
                cases: 62,
                relevant_tools: total,
                recovered_tools: recovered,
                recall,
                mean_latency_ms: mean_ms,
                max_latency_ms: 10,
                candidate_universe_size: 64,
                shortlist_k: k,
                eligible_relevant_tools: total,
                recovered_relevant_tools: recovered,
            }
        }
        let clear = |mode: &str, k: usize| {
            (
                vec![point(mode, k, 1.0, 5.0)],
                vec![point(mode, k, 0.99, 5.0)],
                vec![point(mode, k, 0.96, 5.0)],
            )
        };
        let (p64, p128, p256) = clear("rrf", 24);
        let selected = select_retrieval_operating_point(&p64, &p128, &p256, 0).expect("selection");
        assert_eq!((selected.mode.as_str(), selected.k), ("rrf", 24));
        // Missing 256 gate fails closed.
        let weak_256 = vec![point("rrf", 24, 0.90, 5.0)];
        assert!(select_retrieval_operating_point(&p64, &p128, &weak_256, 0).is_err());
        // Authority violations fail closed.
        assert!(select_retrieval_operating_point(&p64, &p128, &p256, 1).is_err());
        // Smaller K wins over lower latency at larger K.
        let (q64, q128, q256) = clear("bm25", 16);
        let mut all_64 = p64.clone();
        all_64.extend(q64);
        let mut all_128 = p128.clone();
        all_128.extend(q128);
        let mut all_256 = p256.clone();
        all_256.extend(q256);
        let selected =
            select_retrieval_operating_point(&all_64, &all_128, &all_256, 0).expect("selection");
        assert_eq!((selected.mode.as_str(), selected.k), ("bm25", 16));
    }

    /// Predeclared M004 sweep (protocol
    /// `m004-preregistered-operating-point-v1`).
    ///
    /// Consumes the frozen M003 span-packed artifact and its dual
    /// calibration; selects retrieval K/mode on dev universes and a
    /// separate promotion threshold on the predeclared grid; freezes
    /// the operating point; runs the v3 diagnostic once without
    /// feedback. Run explicitly for closure evidence:
    /// `cargo test --locked --features tool-advisor-encoder-training
    /// -p codegg --lib
    /// tool_advisor::operating_point::tests::m004_operating_point_sweep --
    /// --ignored --nocapture`
    #[test]
    #[ignore]
    fn m004_operating_point_sweep() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let ranker_artifact =
            root.join("target/tool-advisor/order-invariance/m003-arms/span-packed.json");
        let encoder_manifest =
            root.join("target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json");
        if !ranker_artifact.exists() || !encoder_manifest.exists() {
            eprintln!("SKIP: M003 artifact or reference encoder assets absent");
            return;
        }
        let device = candle_core::Device::Cpu;
        let out_dir = root.join("target/tool-advisor/order-invariance/m004");
        let sweep = OperatingPointSweepConfig {
            schema_version: OPERATING_POINT_SCHEMA_VERSION,
            protocol: OPERATING_POINT_PROTOCOL.into(),
            ranker_artifact: ranker_artifact.display().to_string(),
            encoder_manifest: encoder_manifest.display().to_string(),
            dataset: "assets/tool-advisor/corpus.jsonl".into(),
            output_dir: out_dir.display().to_string(),
            seed: 0x51AB_1E4D_27D4_EB4F,
            promotion_sample_cases: 12,
            promotion_permutations_per_case: 8,
        };
        let report =
            run_operating_point_selection(&sweep, &device).expect("operating-point selection");
        eprintln!(
            "retrieval: {} K={} (64:{:.4} 128:{:.4} 256:{:.4})",
            report.retrieval.mode,
            report.retrieval.k,
            report.retrieval.recall_64,
            report.retrieval.recall_128,
            report.retrieval.recall_256,
        );
        eprintln!(
            "promotion: threshold={:.2} recall={:.4} no-tool={:.4} irrelevant={:.4} max_promos={} schema_p95={}",
            report.promotion.threshold,
            report.promotion.relevant_recall,
            report.promotion.no_tool_promotion_rate,
            report.promotion.irrelevant_promotion_rate,
            report.promotion.max_promotions_observed,
            report.promotion.schema_p95_bytes,
        );
        eprintln!(
            "promotion order: consistency={:.3} crossings={}; resources: retrieval p50/p95={:.1}/{:.1}ms ranking p50/p95={:.1}/{:.1}ms",
            report.promotion_order.identity_consistency,
            report.promotion_order.threshold_crossing_cases,
            report.resources.retrieval_p50_ms,
            report.resources.retrieval_p95_ms,
            report.resources.ranking_p50_ms,
            report.resources.ranking_p95_ms,
        );
        eprintln!(
            "train-128 confirmation recall: {:.4}; v3 diagnostic: retrieval 64/128={:.4}/{:.4} promotion recall={:.4} no-tool={:.4} irrelevant={:.4}",
            report.train_confirmation_recall,
            report.v3_diagnostic.retrieval_recall_64,
            report.v3_diagnostic.retrieval_recall_128,
            report.v3_diagnostic.promotion_recall,
            report.v3_diagnostic.promotion_no_tool_rate,
            report.v3_diagnostic.promotion_irrelevant_rate,
        );
        std::fs::write(
            out_dir.join("m004-operating-point.json"),
            serde_json::to_vec_pretty(&report).expect("serialize"),
        )
        .expect("write report");
        std::fs::write(
            out_dir.join("advisor-operating-point.json"),
            serde_json::to_vec_pretty(&report.operating_point).expect("serialize"),
        )
        .expect("write operating point");
        eprintln!("receipts written under {}", out_dir.display());
    }
}
