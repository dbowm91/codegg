//! R001 retrieval-architecture experiment: miss diagnostics and preregistration.
//!
//! Follow-up to the order-invariance M004 negative close: no K<=32
//! configuration of the four predeclared modes clears the large-catalog
//! recall gates on dev, and the miss is systematic (normalized-union
//! plateaus at 0.9444 from K16 with no K-scaling) rather than a cutoff
//! edge. This module attributes those misses and freezes the M002/M003
//! selection contract so the extended sweep has zero unpreregistered
//! degrees of freedom.
//!
//! Scope boundary (M001): pure diagnostics over dev-only fixtures plus
//! typed preregistration constants. No encoder workload runs here, no
//! retriever variant is implemented, and no gate is relaxed. The
//! extended frontier sweep runs once in M003 under the exact command
//! recorded in [`PREREG_SWEEP_COMMAND`]:
//! `cargo test --locked --features tool-advisor-encoder-training -p
//! codegg --lib --
//! tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep
//! --ignored --nocapture`

use super::context_v2::AdvisorContextV2;
use super::operating_point::{
    expand_universe, promotion_case_views, promotion_order_evidence,
    promotion_outcome_for_threshold, promotion_threshold_grid, select_promotion_threshold,
    PromotionOrderEvidence, PromotionOutcome,
};
use super::sequence_encoder::{CandleBertSequenceEncoder, PoolingStrategy};
use super::sequence_ranking::{
    load_artifact as load_ranker_artifact, SequenceRanker, RANKING_ARCHITECTURE_SPAN_PACKED,
};
use super::sequence_retrieval::RETRIEVAL_SCHEMA_VERSION;
use super::{
    baseline_prediction, dataset_fingerprint, load_cases, partition_cases, RankedCandidate,
    ToolAdvisorCandidate, ToolAdvisorCase,
};
use crate::tool::catalog::SearchMode;
use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

// ---- preregistered selection contract ----

/// Preregistered protocol name for the M003 extended frontier sweep.
pub const RETRIEVAL_ARCH_PROTOCOL: &str = "r001-preregistered-retrieval-architecture-v1";
/// Schema version for [`Preregistration`] and the M003 sweep config.
pub const RETRIEVAL_ARCH_SCHEMA_VERSION: u16 = 1;
/// Code version of the universe-expansion input. Any change to the
/// expansion logic must bump this so [`fixture_fingerprint`] mismatches
/// instead of silently rebinding the sweep to new fixtures.
pub const EXPANSION_CODE_VERSION: u16 = 1;

/// Preregistered candidate universes (deferred tools per case).
pub const PREREG_UNIVERSES: [usize; 3] = [64, 128, 256];
/// Primary shortlists. The M004 gates must clear here first.
pub const PREREG_PRIMARY_KS: [usize; 3] = [16, 24, 32];
/// Bounded extended shortlists, measurable only with all budgets met.
pub const PREREG_EXTENDED_KS: [usize; 2] = [48, 64];

/// Preregistered recall gates, unchanged from M004.
pub const GATE_RECALL_64: f64 = 0.99;
pub const GATE_RECALL_128: f64 = 0.98;
pub const GATE_RECALL_256: f64 = 0.95;

/// Fusion variant names. `weighted-union` at alpha 0.5 reproduces the
/// M004 equally weighted normalized union for comparability.
pub const FUSION_VARIANTS: [&str; 3] = ["weighted-union", "rrf", "max"];
/// Semantic weight grid for `weighted-union`. Alpha 0.0 is BM25-only,
/// 1.0 is semantic-only, so the grid adjudicates whether the four M004
/// misses are dual-signal or merely fusion-weight sensitive.
pub const FUSION_ALPHAS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// RRF rank-constant grid. 60.0 is the M004 value.
pub const RRF_KS: [f64; 3] = [30.0, 60.0, 120.0];
/// Query/descriptor pooling ablation for the semantic side.
pub const POOLING_VARIANTS: [&str; 2] = ["mean", "cls"];
/// Candidate-pool sizes for the two-stage re-rank (union top-N through
/// the frozen M003 span-packed ranker down to top-K).
pub const RERANK_CANDIDATE_NS: [usize; 2] = [48, 64];

/// Retrieval latency budget: p95 per frontier point.
pub const BUDGET_RETRIEVAL_P95_MS: f64 = 5_000.0;
/// Retrieval latency budget: max per frontier point.
pub const BUDGET_RETRIEVAL_MAX_MS: u128 = 30_000;
/// Two-stage re-rank budget: mean ranker cost per case.
pub const BUDGET_RERANK_PER_CASE_MS: f64 = 5_000.0;
/// Process-cold encoder load budget, preserved from prior envelopes.
pub const BUDGET_COLD_LOAD_P95_S: f64 = 10.0;
/// Encoder weight budget, preserved from prior envelopes.
pub const BUDGET_ENCODER_WEIGHTS_MIB: u64 = 128;
/// Total sweep wall-clock budget; the M004 precedent took ~65 min with
/// first-embedding-load outliers, so M003 must cache or budget load.
pub const BUDGET_SWEEP_WALLCLOCK_S: u64 = 7_200;
/// Promotion schema budget, unchanged from M004.
pub const BUDGET_SCHEMA_P95_BYTES: usize = 16 * 1024;

/// Frozen selection inputs. The ranker is the M003 span-packed artifact;
/// the encoder manifest is the reference MiniLM asset M004 used.
pub const PREREG_DATASET: &str = "assets/tool-advisor/corpus.jsonl";
pub const PREREG_RANKER_ARTIFACT: &str =
    "target/tool-advisor/order-invariance/m003-arms/span-packed.json";
pub const PREREG_ENCODER_MANIFEST: &str =
    "target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json";
pub const PREREG_OUTPUT_DIR: &str = "target/tool-advisor/retrieval-architecture";
/// Deterministic sweep seed (ASCII "retriev1").
pub const PREREG_SEED: u64 = 0x7275_7472_6965_7631;

/// Exact M003 sweep command, recorded here so the closure can quote it.
pub const PREREG_SWEEP_COMMAND: &str = "cargo test --locked \
     --features tool-advisor-encoder-training -p codegg --lib -- \
     tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep \
     --ignored --nocapture";

/// M004 committed reference: BM25 recovered tools per universe (flat
/// across K=16/24/32). The M001 diagnostic must reproduce these counts.
pub const M004_BM25_RECOVERED: usize = 52;
/// M004 committed reference: eligible relevant tools per universe.
pub const M004_ELIGIBLE_RELEVANT: usize = 72;
/// M004 committed reference: dev cases per universe.
pub const M004_DEV_CASES: usize = 62;
/// M004 committed reference: best normalized-union recall (ceiling).
pub const M004_UNION_BEST_RECALL: f64 = 0.9444;

/// Expected dev-partition fingerprint of the frozen corpus under
/// [`partition_cases`], cross-checked against the M003 selection
/// receipt. Any corpus or partition-logic change fails this-tripwire
/// instead of silently rebinding the sweep.
pub const EXPECTED_DEV_PARTITION_FINGERPRINT: &str =
    "b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9";

/// Expected live fixture fingerprint from [`fixture_fingerprint`],
/// recorded at M001 close. The M003 sweep recomputes it live and
/// refuses any mismatch, so any corpus, receipt, or expansion-code
/// drift fails closed.
pub const EXPECTED_FIXTURE_FINGERPRINT: &str =
    "e631cd2f3398d8a1a690fafa7fa7eff5e18454b1dfe64596d57026fd5ed184a7";

/// Recall gates for one operating-point candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateThresholds {
    pub recall_64: f64,
    pub recall_128: f64,
    pub recall_256: f64,
}

/// Resource budgets the extended-K branch must additionally satisfy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetLimits {
    pub retrieval_p95_ms: f64,
    pub retrieval_max_ms: u128,
    pub rerank_per_case_ms: f64,
    pub cold_load_p95_s: f64,
    pub encoder_weights_mib: u64,
    pub sweep_wallclock_s: u64,
    pub schema_p95_bytes: usize,
}

/// Measured resource evidence for one sweep candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetEvidence {
    pub retrieval_p95_ms: f64,
    pub retrieval_max_ms: u128,
    pub rerank_per_case_ms: f64,
    pub cold_load_p95_s: f64,
    pub encoder_weights_mib: u64,
    pub sweep_wallclock_s: u64,
    pub schema_p95_bytes: usize,
}

/// Frozen M002/M003 selection contract. The sweep fingerprint is
/// derived from this struct but never stored inside it: no
/// self-referential SHA is allowed in the protocol hash, and the
/// implementation commit SHA is passed separately at sweep time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub dataset: String,
    pub ranker_artifact: String,
    pub encoder_manifest: String,
    pub output_dir: String,
    pub seed: u64,
    pub universes: Vec<usize>,
    pub primary_ks: Vec<usize>,
    pub extended_ks: Vec<usize>,
    pub fusion_variants: Vec<String>,
    pub fusion_alphas: Vec<f64>,
    pub rrf_ks: Vec<f64>,
    pub pooling_variants: Vec<String>,
    pub rerank_candidate_ns: Vec<usize>,
    pub gates: GateThresholds,
    pub budgets: BudgetLimits,
    pub sweep_command: String,
    pub checkpoint_dir: String,
    /// Expected fixture fingerprint from [`fixture_fingerprint`],
    /// recorded at M001 close. The M003 sweep recomputes it live and
    /// refuses any mismatch.
    pub expected_fixture_fingerprint: String,
}

/// Build the frozen preregistration. Every M002/M003 degree of freedom
/// is a typed value here; adding one later requires a roadmap
/// amendment, not a silent default.
pub fn preregistration(expected_fixture_fingerprint: &str) -> Preregistration {
    Preregistration {
        schema_version: RETRIEVAL_ARCH_SCHEMA_VERSION,
        protocol: RETRIEVAL_ARCH_PROTOCOL.into(),
        dataset: PREREG_DATASET.into(),
        ranker_artifact: PREREG_RANKER_ARTIFACT.into(),
        encoder_manifest: PREREG_ENCODER_MANIFEST.into(),
        output_dir: PREREG_OUTPUT_DIR.into(),
        seed: PREREG_SEED,
        universes: PREREG_UNIVERSES.into(),
        primary_ks: PREREG_PRIMARY_KS.into(),
        extended_ks: PREREG_EXTENDED_KS.into(),
        fusion_variants: FUSION_VARIANTS.iter().map(ToString::to_string).collect(),
        fusion_alphas: FUSION_ALPHAS.into(),
        rrf_ks: RRF_KS.into(),
        pooling_variants: POOLING_VARIANTS.iter().map(ToString::to_string).collect(),
        rerank_candidate_ns: RERANK_CANDIDATE_NS.into(),
        gates: GateThresholds {
            recall_64: GATE_RECALL_64,
            recall_128: GATE_RECALL_128,
            recall_256: GATE_RECALL_256,
        },
        budgets: BudgetLimits {
            retrieval_p95_ms: BUDGET_RETRIEVAL_P95_MS,
            retrieval_max_ms: BUDGET_RETRIEVAL_MAX_MS,
            rerank_per_case_ms: BUDGET_RERANK_PER_CASE_MS,
            cold_load_p95_s: BUDGET_COLD_LOAD_P95_S,
            encoder_weights_mib: BUDGET_ENCODER_WEIGHTS_MIB,
            sweep_wallclock_s: BUDGET_SWEEP_WALLCLOCK_S,
            schema_p95_bytes: BUDGET_SCHEMA_P95_BYTES,
        },
        sweep_command: PREREG_SWEEP_COMMAND.into(),
        checkpoint_dir: PREREG_OUTPUT_DIR.into(),
        expected_fixture_fingerprint: expected_fixture_fingerprint.into(),
    }
}

/// Sweep fingerprint over the serialized preregistration. The output
/// must never appear inside the serialized struct itself.
pub fn prereg_fingerprint(prereg: &Preregistration) -> Result<String> {
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(prereg).context("serialize preregistration")?,
    )))
}

/// Fixture fingerprint over the exact M003 inputs: selection-receipt
/// bytes, dev-partition fingerprint, and expansion code version.
pub fn fixture_fingerprint_for(
    selection_receipt_bytes: &[u8],
    dev_partition_fingerprint: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"retrieval-architecture-fixtures-v1\n");
    hasher.update(selection_receipt_bytes);
    hasher.update(b"\n");
    hasher.update(dev_partition_fingerprint.as_bytes());
    hasher.update(b"\n");
    hasher.update(EXPANSION_CODE_VERSION.to_le_bytes());
    hex::encode(hasher.finalize())
}

/// Live fixture fingerprint: reads the committed M003 selection receipt
/// and derives the dev partition from the frozen corpus. No v3/v4 case
/// is read.
pub fn fixture_fingerprint() -> Result<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let receipt =
        std::fs::read(root.join("assets/tool-advisor/order-invariance-m003-selection.json"))
            .context("read M003 selection receipt")?;
    let cases = load_cases(Some(&root.join(PREREG_DATASET))).context("load frozen corpus")?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases).context("fingerprint dev partition")?;
    Ok(fixture_fingerprint_for(&receipt, &dev_fp))
}

/// Accept only the preregistered fixture fingerprint.
pub fn verify_fixture_fingerprint(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        return Err(anyhow!(
            "fixture fingerprint mismatch: sweep inputs changed since preregistration"
        ));
    }
    Ok(())
}

/// Dev-partition fingerprint tripwire: the frozen corpus under
/// [`partition_cases`] must reproduce the M003 dev partition.
pub fn dev_partition_tripwire() -> Result<String> {
    let cases = load_cases(None).context("load builtin corpus")?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases).context("fingerprint dev partition")?;
    if dev_fp != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!(
            "dev partition fingerprint changed: corpus or partition logic drifted"
        ));
    }
    Ok(dev_fp)
}

// ---- miss attribution ----

/// Cause classification for one missed relevant tool at shortlist K.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MissCause {
    /// Beyond K in both BM25 and semantic orderings: a dual-signal
    /// scoring gap, not a cutoff or weighting artifact.
    DualSignalMiss,
    /// Reaches K in exactly one signal but the fusion drops it: the
    /// outcome is fusion-weight sensitive and the preregistered alpha
    /// grid adjudicates it.
    FusionWeightSensitive,
    /// Reaches K in both signals but the fusion still drops it.
    FusionMarginMiss,
    /// Beyond K in BM25 with no semantic ordering supplied: the
    /// lexical signal alone cannot reach it.
    LexicalGap,
    /// A needed ordering is absent and the supplied ones do not
    /// determine the verdict.
    InsufficientEvidence,
}

impl MissCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DualSignalMiss => "dual-signal-miss",
            Self::FusionWeightSensitive => "fusion-weight-sensitive",
            Self::FusionMarginMiss => "fusion-margin-miss",
            Self::LexicalGap => "lexical-gap",
            Self::InsufficientEvidence => "insufficient-evidence",
        }
    }
}

/// Per-tool attribution for a relevant tool missed at shortlist K.
/// Ranks are 1-based positions in the supplied full-universe orderings
/// (`None` when the tool is unscored). `margin_to_boundary` is the
/// fused score minus the K-th fused score (`None` when the tool has no
/// fused score).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissAttribution {
    pub tool_name: String,
    pub bm25_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub fused_rank: Option<usize>,
    pub margin_to_boundary: Option<f64>,
    pub cause: MissCause,
}

fn rank_of(ordering: &[RankedCandidate], tool: &str) -> Option<usize> {
    ordering
        .iter()
        .position(|entry| entry.name == tool)
        .map(|index| index + 1)
}

fn score_of(ordering: &[RankedCandidate], tool: &str) -> Option<f64> {
    ordering
        .iter()
        .find(|entry| entry.name == tool)
        .map(|entry| entry.score)
}

/// Attribute every relevant tool missed by the fused ordering at
/// shortlist `k`. Tools the fusion recovers produce no attribution.
/// Empty relevance yields no attributions.
pub fn attribute_misses(
    relevant: &BTreeSet<String>,
    bm25: &[RankedCandidate],
    semantic: Option<&[RankedCandidate]>,
    fused: &[RankedCandidate],
    k: usize,
) -> Vec<MissAttribution> {
    let boundary = fused.get(k.saturating_sub(1)).map(|entry| entry.score);
    let mut missed: Vec<MissAttribution> = relevant
        .iter()
        .filter(|tool| rank_of(fused, tool).is_none_or(|rank| rank > k))
        .map(|tool| {
            let bm25_rank = rank_of(bm25, tool);
            let semantic_rank = semantic.and_then(|ordering| rank_of(ordering, tool));
            let fused_rank = rank_of(fused, tool);
            let margin_to_boundary = match (score_of(fused, tool), boundary) {
                (Some(score), Some(edge)) => Some(score - edge),
                _ => None,
            };
            let bm25_reaches = bm25_rank.is_some_and(|rank| rank <= k);
            let semantic_reaches = semantic_rank.is_some_and(|rank| rank <= k);
            let cause = match (bm25_reaches, semantic_reaches, semantic.is_some()) {
                (false, false, true) => MissCause::DualSignalMiss,
                (true, true, _) => MissCause::FusionMarginMiss,
                (false, false, false) => MissCause::LexicalGap,
                (_, _, false) if bm25_reaches => MissCause::FusionMarginMiss,
                _ => MissCause::FusionWeightSensitive,
            };
            // No usable ordering at all carries no evidence beyond the
            // miss itself. A non-empty BM25 ordering that omits the tool
            // is genuine lexical-gap evidence.
            let cause = if semantic.is_none() && bm25.is_empty() {
                MissCause::InsufficientEvidence
            } else {
                cause
            };
            MissAttribution {
                tool_name: tool.clone(),
                bm25_rank,
                semantic_rank,
                fused_rank,
                margin_to_boundary,
                cause,
            }
        })
        .collect();
    missed.sort_by(|left, right| left.tool_name.cmp(&right.tool_name));
    missed
}

// ---- reference fusion math ----

fn normalize_scores(values: &[(String, f64)]) -> Vec<(String, f64)> {
    if values.is_empty() {
        return Vec::new();
    }
    let min = values
        .iter()
        .map(|(_, score)| *score)
        .fold(f64::INFINITY, f64::min);
    let max = values
        .iter()
        .map(|(_, score)| *score)
        .fold(f64::NEG_INFINITY, f64::max);
    values
        .iter()
        .map(|(name, score)| {
            let value = if (max - min).abs() <= f64::EPSILON {
                1.0
            } else {
                (score - min) / (max - min)
            };
            (name.clone(), value)
        })
        .collect()
}

fn ranked_names(scores: &BTreeMap<String, f64>) -> Vec<RankedCandidate> {
    let mut entries: Vec<RankedCandidate> = scores
        .iter()
        .map(|(name, score)| RankedCandidate {
            name: name.clone(),
            score: *score,
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    entries
}

/// Reference alpha-weighted normalized union. Alpha is the semantic
/// weight; 0.5 reproduces the M004 equally weighted union. Normative
/// for M002: the retriever must match these vectors exactly.
pub fn fuse_weighted_union(
    bm25: &[(String, f64)],
    semantic: &[(String, f64)],
    alpha: f64,
) -> Vec<RankedCandidate> {
    let bm25 = normalize_scores(bm25);
    let semantic = normalize_scores(semantic);
    let mut scores = BTreeMap::new();
    for (name, score) in bm25 {
        *scores.entry(name).or_insert(0.0) += (1.0 - alpha) * score;
    }
    for (name, score) in semantic {
        *scores.entry(name).or_insert(0.0) += alpha * score;
    }
    ranked_names(&scores)
}

fn rank_index(values: &[(String, f64)]) -> BTreeMap<String, usize> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    sorted
        .into_iter()
        .enumerate()
        .map(|(index, (name, _))| (name, index))
        .collect()
}

/// Reference RRF with an explicit rank constant. RRF_K 60.0 reproduces
/// the M004 fusion. Normative for M002.
pub fn fuse_rrf_with_k(
    bm25: &[(String, f64)],
    semantic: &[(String, f64)],
    rrf_k: f64,
) -> Vec<RankedCandidate> {
    let bm25_ranks = rank_index(bm25);
    let semantic_ranks = rank_index(semantic);
    let mut names: BTreeSet<String> = bm25_ranks.keys().cloned().collect();
    names.extend(semantic_ranks.keys().cloned());
    let scores: BTreeMap<String, f64> = names
        .into_iter()
        .map(|name| {
            let mut total = 0.0;
            if let Some(rank) = bm25_ranks.get(&name) {
                total += 1.0 / (rrf_k + *rank as f64 + 1.0);
            }
            if let Some(rank) = semantic_ranks.get(&name) {
                total += 1.0 / (rrf_k + *rank as f64 + 1.0);
            }
            (name, total)
        })
        .collect();
    ranked_names(&scores)
}

/// Reference max fusion over normalized signals. Normative for M002.
pub fn fuse_max(bm25: &[(String, f64)], semantic: &[(String, f64)]) -> Vec<RankedCandidate> {
    let bm25 = normalize_scores(bm25);
    let semantic = normalize_scores(semantic);
    let mut scores = BTreeMap::new();
    for (name, score) in bm25.into_iter().chain(semantic) {
        scores
            .entry(name)
            .and_modify(|current: &mut f64| *current = (*current).max(score))
            .or_insert(score);
    }
    ranked_names(&scores)
}

// ---- budgets and extended-K gating ----

/// Fail closed on any budget breach. The extended-K branch cannot
/// satisfy selection unless every budget passes.
pub fn verify_budgets(evidence: &BudgetEvidence, limits: &BudgetLimits) -> Result<()> {
    if evidence.retrieval_p95_ms > limits.retrieval_p95_ms {
        return Err(anyhow!("retrieval p95 exceeds budget"));
    }
    if evidence.retrieval_max_ms > limits.retrieval_max_ms {
        return Err(anyhow!("retrieval max latency exceeds budget"));
    }
    if evidence.rerank_per_case_ms > limits.rerank_per_case_ms {
        return Err(anyhow!("re-rank cost exceeds budget"));
    }
    if evidence.cold_load_p95_s > limits.cold_load_p95_s {
        return Err(anyhow!("cold load exceeds budget"));
    }
    if evidence.encoder_weights_mib > limits.encoder_weights_mib {
        return Err(anyhow!("encoder weights exceed budget"));
    }
    if evidence.sweep_wallclock_s > limits.sweep_wallclock_s {
        return Err(anyhow!("sweep wall-clock exceeds budget"));
    }
    if evidence.schema_p95_bytes > limits.schema_p95_bytes {
        return Err(anyhow!("schema bytes exceed budget"));
    }
    Ok(())
}

/// Extended-K acceptance: the preregistered gates must clear at the
/// extended shortlist AND every budget must pass. Either failure
/// closes negatively instead of raising the candidate budget.
pub fn extended_selection_allowed(
    recall_64: f64,
    recall_128: f64,
    recall_256: f64,
    evidence: &BudgetEvidence,
    limits: &BudgetLimits,
) -> Result<()> {
    if !(recall_64 >= GATE_RECALL_64
        && recall_128 >= GATE_RECALL_128
        && recall_256 >= GATE_RECALL_256)
    {
        return Err(anyhow!(
            "extended-K recalls {recall_64:.4}/{recall_128:.4}/{recall_256:.4} miss \
             {GATE_RECALL_64}/{GATE_RECALL_128}/{GATE_RECALL_256}"
        ));
    }
    verify_budgets(evidence, limits)
}

// ---- checkpoint binding and authority hygiene ----

/// Fingerprint-bound checkpoint path for one retrieval universe.
pub fn checkpoint_path(output_dir: &str, universe_size: usize) -> PathBuf {
    PathBuf::from(output_dir).join(format!("r001-checkpoint-frontier-{universe_size}.json"))
}

/// Resume only on fingerprint and universe match; otherwise recompute.
pub fn checkpoint_accepts(
    stored_fingerprint: &str,
    stored_universe: usize,
    expected_fingerprint: &str,
    expected_universe: usize,
) -> bool {
    stored_fingerprint == expected_fingerprint && stored_universe == expected_universe
}

/// Names of the already authority-filtered deferred universe.
pub fn eligible_deferred_names(case: &ToolAdvisorCase) -> BTreeSet<String> {
    case.candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.clone())
        .collect()
}

/// Fail closed when an ordering names a tool outside the eligible
/// deferred universe.
pub fn verify_universe(ordering: &[RankedCandidate], eligible: &BTreeSet<String>) -> Result<()> {
    for entry in ordering {
        if !eligible.contains(&entry.name) {
            return Err(anyhow!(
                "retrieval ordering names {} outside the eligible deferred universe",
                entry.name
            ));
        }
    }
    Ok(())
}

// ---- BM25 diagnostic (no encoder workload) ----

/// Full-universe BM25 ordering over the eligible deferred set,
/// mirroring the `sequence_retrieval` BM25 path without K truncation:
/// catalog BM25 scores, deferred-only filter, zero-score completion,
/// score-descending with name tiebreaks.
pub fn bm25_ordering(case: &ToolAdvisorCase) -> Vec<RankedCandidate> {
    let prediction = baseline_prediction(case, SearchMode::BM25);
    let allowed = eligible_deferred_names(case);
    let mut ranked: Vec<RankedCandidate> = prediction
        .ranked
        .into_iter()
        .filter(|entry| allowed.contains(&entry.name))
        .collect();
    let known: BTreeSet<String> = ranked.iter().map(|entry| entry.name.clone()).collect();
    for name in &allowed {
        if !known.contains(name) {
            ranked.push(RankedCandidate {
                name: name.clone(),
                score: 0.0,
            });
        }
    }
    ranked.sort_by(|left, right| left.name.cmp(&right.name));
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    ranked
}

/// BM25 recovered/eligible relevant counts over expanded cases at
/// shortlist `k`, mirroring the frontier recall accounting.
pub fn bm25_recall_at_k(cases: &[ToolAdvisorCase], k: usize) -> (usize, usize) {
    let mut eligible = 0usize;
    let mut recovered = 0usize;
    for case in cases {
        let allowed = eligible_deferred_names(case);
        let expected: Vec<&String> = case
            .relevance
            .keys()
            .filter(|name| allowed.contains(*name))
            .collect();
        if expected.is_empty() {
            continue;
        }
        let top: BTreeSet<String> = bm25_ordering(case)
            .into_iter()
            .take(k)
            .map(|entry| entry.name)
            .collect();
        eligible += expected.len();
        recovered += expected
            .iter()
            .filter(|name| top.contains(name.as_str()))
            .count();
    }
    (recovered, eligible)
}

// ---- M002 variant space (fusion, pooling, lexical, re-rank) ----
//
// M001 froze the selection contract; this section implements exactly the
// preregistered variant axes behind the existing authority boundary. No
// selection, gating, or threshold logic lives here (that belongs to M003):
// - `VariantSelection`: typed grid parameters; unknown values fail closed.
// - `select_coarse_ordering`: pure fusion dispatch that CALLS the M001
//   normative reference functions (`fuse_weighted_union`,
//   `fuse_rrf_with_k`, `fuse_max`); reimplementing the math here is a defect.
// - `VariantRetriever`: pooling-parameterized coarse retrieval over
//   deferred-only descriptors with BM25 fallback on any encoder failure.
// - `advisor_lexical_ordering`: the advisor-side lexical signal, reusing
//   catalog BM25 machinery without altering it.
// - `rerank_pool`: two-stage re-rank through the frozen M003 span-packed
//   ranker with per-case cost accounting.

/// Typed M002 variant parameters. Every field must be a preregistered
/// M001 grid value (`FUSION_VARIANTS`, `FUSION_ALPHAS`, `RRF_KS`,
/// `POOLING_VARIANTS`, `RERANK_CANDIDATE_NS`); anything else fails closed
/// at construction. Grid floats compare exactly: the preregistered values
/// are exact binary fractions, so only exact matches validate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariantSelection {
    pub fusion: String,
    pub alpha: f64,
    pub rrf_k: f64,
    pub pooling: String,
    pub rerank_n: usize,
}

impl VariantSelection {
    pub fn new(
        fusion: &str,
        alpha: f64,
        rrf_k: f64,
        pooling: &str,
        rerank_n: usize,
    ) -> Result<Self> {
        if !FUSION_VARIANTS.contains(&fusion) {
            return Err(anyhow!("unpreregistered fusion variant {fusion}"));
        }
        if !FUSION_ALPHAS.contains(&alpha) {
            return Err(anyhow!("unpreregistered fusion alpha {alpha}"));
        }
        if !RRF_KS.contains(&rrf_k) {
            return Err(anyhow!("unpreregistered RRF constant {rrf_k}"));
        }
        if !POOLING_VARIANTS.contains(&pooling) {
            return Err(anyhow!("unpreregistered pooling variant {pooling}"));
        }
        if !RERANK_CANDIDATE_NS.contains(&rerank_n) {
            return Err(anyhow!("unpreregistered re-rank pool size {rerank_n}"));
        }
        Ok(Self {
            fusion: fusion.into(),
            alpha,
            rrf_k,
            pooling: pooling.into(),
            rerank_n,
        })
    }

    /// Map the validated pooling name to the encoder strategy.
    /// (`POOLING_VARIANTS`, preregistered in M001.)
    pub fn pooling_strategy(&self) -> Result<PoolingStrategy> {
        parse_pooling(&self.pooling)
    }

    /// Canonical M003 frontier mode name for this grid point. The mapping
    /// is injective over the preregistered grid: every field is named, so
    /// no two grid points share a mode string and `select_retrieval_point`
    /// keeps its exact-identity semantics on variant points.
    pub fn point_mode(&self) -> String {
        format!(
            "r001-{}-a{}-k{}-p{}-n{}",
            self.fusion, self.alpha, self.rrf_k, self.pooling, self.rerank_n
        )
    }
}

/// Parse an encoder pooling strategy, failing closed on anything outside
/// the preregistered `POOLING_VARIANTS`.
pub fn parse_pooling(value: &str) -> Result<PoolingStrategy> {
    match value {
        "mean" => Ok(PoolingStrategy::Mean),
        "cls" => Ok(PoolingStrategy::Cls),
        other => Err(anyhow!("unpreregistered pooling variant {other}")),
    }
}

/// Pure fusion dispatch over pre-scored signal pairs. Delegates to the
/// M001 normative reference functions on identical inputs; the fusion
/// variant comes from a validated [`VariantSelection`].
pub fn select_coarse_ordering(
    bm25: &[(String, f64)],
    semantic: &[(String, f64)],
    selection: &VariantSelection,
) -> Result<Vec<RankedCandidate>> {
    match selection.fusion.as_str() {
        "weighted-union" => Ok(fuse_weighted_union(bm25, semantic, selection.alpha)),
        "rrf" => Ok(fuse_rrf_with_k(bm25, semantic, selection.rrf_k)),
        "max" => Ok(fuse_max(bm25, semantic)),
        other => Err(anyhow!("unpreregistered fusion variant {other}")),
    }
}

/// Advisor-side lexical ordering over the eligible deferred set. This is
/// the M002-named entry point for the lexical signal; it reuses catalog
/// BM25 machinery without altering it by delegating to [`bm25_ordering`].
/// (Preregistered implicitly: the lexical side of every fusion variant.)
pub fn advisor_lexical_ordering(case: &ToolAdvisorCase) -> Vec<RankedCandidate> {
    bm25_ordering(case)
}

fn variant_descriptor(candidate: &ToolAdvisorCandidate) -> String {
    // Parity with `sequence_retrieval`'s private descriptor text: the
    // encoder must see the same surface for variant and M004 paths.
    format!(
        "canonical name: {}; description: {}; category: {}; disclosure: {}",
        candidate.name, candidate.description, candidate.category, candidate.disclosure
    )
}

fn normalized_variant_descriptor(candidate: &ToolAdvisorCandidate) -> String {
    // Parity with `sequence_retrieval`'s private normalization.
    variant_descriptor(candidate)
        .split_whitespace()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn variant_cosine(left: &[f32], right: &[f32]) -> f64 {
    // Parity with `sequence_retrieval`'s private cosine.
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (left, right) in left.iter().zip(right.iter()) {
        dot += f64::from(*left) * f64::from(*right);
        left_norm += f64::from(*left) * f64::from(*left);
        right_norm += f64::from(*right) * f64::from(*right);
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator <= f64::EPSILON {
        0.0
    } else {
        dot / denominator
    }
}

/// Injectable semantic scorer so variant retrieval stays testable without
/// encoder assets: production uses [`EncoderSemanticScorer`], tests use a
/// stub. The scorer only ever sees serialized context strings and
/// descriptor text, never case authority state.
pub trait SemanticScorer {
    fn tokenizer_version(&self) -> String;
    fn encode_query(&self, context: &str, pooling: PoolingStrategy) -> Result<Vec<f32>>;
    fn encode_descriptor(&self, descriptor: &str, pooling: PoolingStrategy) -> Result<Vec<f32>>;
}

/// Production semantic scorer over the reference encoder asset.
pub struct EncoderSemanticScorer<'a> {
    pub encoder: &'a CandleBertSequenceEncoder,
}

impl<'a> EncoderSemanticScorer<'a> {
    pub fn new(encoder: &'a CandleBertSequenceEncoder) -> Self {
        Self { encoder }
    }
}

impl SemanticScorer for EncoderSemanticScorer<'_> {
    fn tokenizer_version(&self) -> String {
        // Parity with `HybridRetriever::new`: same version string, so
        // variant cache keys stay comparable with the M004 cache.
        format!(
            "{}:{}",
            self.encoder.assets.manifest.architecture,
            self.encoder
                .assets
                .manifest
                .hashes
                .get("vocabulary")
                .cloned()
                .unwrap_or_default()
        )
    }

    fn encode_query(&self, context: &str, pooling: PoolingStrategy) -> Result<Vec<f32>> {
        self.encoder.encode_context(context, pooling)
    }

    fn encode_descriptor(&self, descriptor: &str, pooling: PoolingStrategy) -> Result<Vec<f32>> {
        self.encoder.encode_with_pooling("", descriptor, pooling)
    }
}

/// Cache key for variant descriptor embeddings. Experiment-local (not the
/// M004 `RetrievalCacheKey`) because the pooling axis must be part of the
/// key: mean and cls embeddings of one descriptor must never collide. Keys
/// hold descriptor hashes only, never user context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct VariantCacheKey {
    surface_fingerprint: String,
    canonical_name: String,
    normalized_descriptor_hash: String,
    encoder_tokenizer_version: String,
    pooling: String,
}

/// Pooling-parameterized coarse retrieval over deferred-only descriptors.
///
/// Mirrors `HybridRetriever` semantics (deferred-only input, BM25 fallback
/// on any encoder failure so retrieval never becomes an availability gate)
/// with two deliberate differences: the pooling strategy and fusion math
/// come from the preregistered [`VariantSelection`], and the embedding
/// cache is experiment-local so pooling variants cannot collide.
pub struct VariantRetriever<S> {
    scorer: S,
    cache: BTreeMap<VariantCacheKey, Vec<f32>>,
    surface_fingerprint: Option<String>,
}

/// One variant retrieval result. `mode` is the canonical
/// [`VariantSelection::point_mode`] name the M003 sweep records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantRetrieval {
    pub schema_version: u16,
    pub mode: String,
    pub k: usize,
    pub names: Vec<String>,
    pub scores: Vec<f64>,
    pub eligible_count: usize,
    pub fallback_to_bm25: bool,
    pub cache_entries: usize,
    pub retrieval_ms: u128,
}

impl<S: SemanticScorer> VariantRetriever<S> {
    pub fn new(scorer: S) -> Self {
        Self {
            scorer,
            cache: BTreeMap::new(),
            surface_fingerprint: None,
        }
    }

    pub fn retrieve(
        &mut self,
        case: &ToolAdvisorCase,
        surface_fingerprint: &str,
        selection: &VariantSelection,
        k: usize,
    ) -> Result<VariantRetrieval> {
        if k == 0 {
            return Err(anyhow!("retrieval K must be positive"));
        }
        if self.surface_fingerprint.as_deref() != Some(surface_fingerprint) {
            self.cache.clear();
            self.surface_fingerprint = Some(surface_fingerprint.to_string());
        }
        let started = Instant::now();
        let eligible: Vec<&ToolAdvisorCandidate> = case
            .candidates
            .iter()
            .filter(|candidate| candidate.disclosure == "deferred")
            .collect();
        let eligible_count = eligible.len();
        if eligible.is_empty() {
            return Ok(VariantRetrieval {
                schema_version: RETRIEVAL_SCHEMA_VERSION,
                mode: selection.point_mode(),
                k,
                names: Vec::new(),
                scores: Vec::new(),
                eligible_count,
                fallback_to_bm25: false,
                cache_entries: self.cache.len(),
                retrieval_ms: started.elapsed().as_millis(),
            });
        }
        let lexical: Vec<(String, f64)> = advisor_lexical_ordering(case)
            .into_iter()
            .map(|entry| (entry.name, entry.score))
            .collect();
        let fallback = |lexical: &[(String, f64)], cache_entries: usize, started: Instant| {
            let mut truncated = lexical.to_vec();
            sort_scored(&mut truncated);
            truncated.truncate(k.min(eligible_count));
            VariantRetrieval {
                schema_version: RETRIEVAL_SCHEMA_VERSION,
                mode: selection.point_mode(),
                k,
                names: truncated.iter().map(|(name, _)| name.clone()).collect(),
                scores: truncated.iter().map(|(_, score)| *score).collect(),
                eligible_count,
                fallback_to_bm25: true,
                cache_entries,
                retrieval_ms: started.elapsed().as_millis(),
            }
        };
        let pooling = selection.pooling_strategy()?;
        let context = AdvisorContextV2::from_benchmark_context(&case.context).serialize();
        let query = match self.scorer.encode_query(&context, pooling) {
            Ok(query) => query,
            Err(_) => return Ok(fallback(&lexical, self.cache.len(), started)),
        };
        let tokenizer_version = self.scorer.tokenizer_version();
        let mut semantic = Vec::with_capacity(eligible.len());
        for candidate in &eligible {
            let descriptor = variant_descriptor(candidate);
            let key = VariantCacheKey {
                surface_fingerprint: surface_fingerprint.to_string(),
                canonical_name: candidate.name.clone(),
                normalized_descriptor_hash: hex::encode(Sha256::digest(
                    normalized_variant_descriptor(candidate).as_bytes(),
                )),
                encoder_tokenizer_version: tokenizer_version.clone(),
                pooling: selection.pooling.clone(),
            };
            let embedding = match self.cache.get(&key) {
                Some(embedding) => embedding.clone(),
                None => match self.scorer.encode_descriptor(&descriptor, pooling) {
                    Ok(embedding) => {
                        self.cache.insert(key, embedding.clone());
                        embedding
                    }
                    Err(_) => return Ok(fallback(&lexical, self.cache.len(), started)),
                },
            };
            semantic.push((candidate.name.clone(), variant_cosine(&query, &embedding)));
        }
        let mut fused: Vec<(String, f64)> = select_coarse_ordering(&lexical, &semantic, selection)?
            .into_iter()
            .map(|entry| (entry.name, entry.score))
            .collect();
        sort_scored(&mut fused);
        fused.truncate(k.min(eligible_count));
        Ok(VariantRetrieval {
            schema_version: RETRIEVAL_SCHEMA_VERSION,
            mode: selection.point_mode(),
            k,
            names: fused.iter().map(|(name, _)| name.clone()).collect(),
            scores: fused.iter().map(|(_, score)| *score).collect(),
            eligible_count,
            fallback_to_bm25: false,
            cache_entries: self.cache.len(),
            retrieval_ms: started.elapsed().as_millis(),
        })
    }
}

fn sort_scored(scores: &mut [(String, f64)]) {
    // Score-descending with name tiebreaks: the experiment-wide
    // deterministic ordering (parity with `sequence_retrieval`).
    scores.sort_by(|left, right| left.0.cmp(&right.0));
    scores.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
}

/// Chunk scorer for the two-stage re-rank: scores one candidate chunk and
/// reports its forward count (and any architecture-level drops). The
/// production implementation wraps the frozen M003 span-packed ranker;
/// tests use a deterministic stub.
pub trait ChunkRankScorer {
    fn scorer_name(&self) -> &'static str;
    fn max_chunk_candidates(&self) -> usize;
    fn score_chunk(
        &self,
        case: &ToolAdvisorCase,
        candidates: &[ToolAdvisorCandidate],
    ) -> Result<(Vec<RankedCandidate>, usize, usize)>;
}

/// Production re-rank scorer: the frozen M003 span-packed ranker. The
/// artifact is a read-only input: weights and calibration are never
/// retrained here, and ranker scores never enter the descriptor cache.
pub struct FrozenRankerScorer<'a> {
    pub ranker: &'a SequenceRanker,
}

impl<'a> FrozenRankerScorer<'a> {
    /// Bind to the frozen span-packed ranker only. Any other architecture
    /// fails closed: the M003 sweep must measure the selected ranker.
    pub fn new(ranker: &'a SequenceRanker) -> Result<Self> {
        if ranker.manifest.architecture != RANKING_ARCHITECTURE_SPAN_PACKED {
            return Err(anyhow!(
                "re-rank requires the frozen span-packed ranker, found {}",
                ranker.manifest.architecture
            ));
        }
        Ok(Self { ranker })
    }
}

impl ChunkRankScorer for FrozenRankerScorer<'_> {
    fn scorer_name(&self) -> &'static str {
        "frozen-span-packed"
    }

    fn max_chunk_candidates(&self) -> usize {
        self.ranker.manifest.max_candidates.max(1)
    }

    fn score_chunk(
        &self,
        case: &ToolAdvisorCase,
        candidates: &[ToolAdvisorCandidate],
    ) -> Result<(Vec<RankedCandidate>, usize, usize)> {
        let mut chunk_case = case.clone();
        chunk_case.candidates = candidates.to_vec();
        let (prediction, dropped, forwards) = self.ranker.predict_case(&chunk_case)?;
        Ok((prediction.ranked, forwards, dropped))
    }
}

/// Two-stage re-rank accounting for one pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankReport {
    pub scorer: String,
    pub pool_size: usize,
    pub promoted_k: usize,
    pub promoted: Vec<RankedCandidate>,
    pub chunks: usize,
    pub forwards: usize,
    pub dropped: usize,
    pub elapsed_ms: u128,
}

/// Re-rank a coarse union pool through the chunk scorer down to top `k`.
///
/// The pool is name-sorted before chunking so chunk boundaries (and hence
/// scores) are order-independent: a permuted pool promotes the identical
/// set. Pool names are revalidated against the eligible deferred universe
/// before scoring; outsiders fail closed. Chunking is honest, not silent:
/// the frozen ranker scores at most `max_chunk_candidates` per forward
/// (its manifest caps at 16, below the preregistered pools of 48/64), so
/// every chunk and forward is counted and chunk-local scores merge by
/// global sort. No selection or gating happens here.
pub fn rerank_pool<S: ChunkRankScorer>(
    scorer: &S,
    case: &ToolAdvisorCase,
    pool_names: &[String],
    k: usize,
) -> Result<RerankReport> {
    if k == 0 {
        return Err(anyhow!("re-rank K must be positive"));
    }
    if pool_names.is_empty() {
        return Err(anyhow!("re-rank pool must be non-empty"));
    }
    let eligible = eligible_deferred_names(case);
    let mut pool: Vec<&ToolAdvisorCandidate> = Vec::with_capacity(pool_names.len());
    for name in pool_names {
        if !eligible.contains(name) {
            return Err(anyhow!(
                "re-rank pool names {name} outside the eligible deferred universe"
            ));
        }
        let candidate = case
            .candidates
            .iter()
            .find(|candidate| &candidate.name == name)
            .ok_or_else(|| anyhow!("re-rank pool names unknown candidate {name}"))?;
        pool.push(candidate);
    }
    pool.sort_by(|left, right| left.name.cmp(&right.name));
    let chunk_size = scorer.max_chunk_candidates().max(1);
    let started = Instant::now();
    let mut merged: Vec<RankedCandidate> = Vec::new();
    let mut chunks = 0usize;
    let mut forwards = 0usize;
    let mut dropped = 0usize;
    for chunk in pool.chunks(chunk_size) {
        let owned: Vec<ToolAdvisorCandidate> = chunk.iter().map(|c| (*c).clone()).collect();
        let (ranked, chunk_forwards, chunk_dropped) = scorer.score_chunk(case, &owned)?;
        merged.extend(ranked);
        chunks += 1;
        forwards += chunk_forwards;
        dropped += chunk_dropped;
    }
    let mut scored: Vec<(String, f64)> = merged
        .into_iter()
        .map(|entry| (entry.name, entry.score))
        .collect();
    sort_scored(&mut scored);
    scored.truncate(k.min(pool.len()));
    Ok(RerankReport {
        scorer: scorer.scorer_name().into(),
        pool_size: pool.len(),
        promoted_k: k,
        promoted: scored
            .into_iter()
            .map(|(name, score)| RankedCandidate { name, score })
            .collect(),
        chunks,
        forwards,
        dropped,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

// ---- M003 extended frontier sweep ----
//
// One-shot adjudication of the preregistered variant grid on dev. The
// sweep measures every M002 variant path over the universe/K grid,
// selects the smallest/cheapest point clearing the M004 gates, and
// re-attaches the unchanged M004 promotion separator at the frozen
// point. Measurement strategy (explicit, no silent subsampling):
// - Coarse: all 180 grid `VariantSelection`s are retrieved directly
//   per case at [`SWEEP_FULL_K`]; every smaller cut derives from the
//   measured ordering. `rerank_n` is inert for coarse orderings
//   (`retrieve` never reads it) but both values are looped, so every
//   coarse mode record is a direct measurement.
// - Re-rank: 36 distinct arms (9 fusion-settings x 2 poolings x 2 pool
//   sizes) are measured through [`rerank_pool`]. Inert-axis duplicates
//   (alpha outside weighted-union, `rrf_k` outside rrf) share the
//   identical deterministic pool and ranker path, so their records fan
//   out from the arm measurement with explicit `rerank_arm`
//   provenance instead of re-running tens of thousands of identical
//   ranker forwards. `rerank_pool` is deterministic (name-sorted
//   chunking, CPU ranker forwards), so fan-out is exact, not sampled.
// - Latency is measured at the full-ordering call: BM25, query
//   encoding, descriptor encoding, and fusion are all K-independent,
//   so the full-K timing fairly bounds every smaller cut of the same
//   ordering. This is conservative in the budget-safe direction and is
//   recorded as such, never silently attributed per-K.

/// Every K the sweep cuts orderings at: primary gates first, bounded
/// extended shortlists only with all budgets met.
pub const SWEEP_KS: [usize; 5] = [16, 24, 32, 48, 64];
/// Full-ordering retrieval K: the largest sweep cut, so every smaller
/// cut derives from one measured ordering per (case, variant).
pub const SWEEP_FULL_K: usize = 64;
/// M004 promotion-evidence sample, reused unchanged with the separator.
pub const PROMOTION_EVIDENCE_SAMPLE_CASES: usize = 12;
pub const PROMOTION_EVIDENCE_PERMUTATIONS: usize = 8;

/// All 180 grid variant selections in deterministic order
/// (fusion x alpha x rrf_k x pooling x rerank_n). Cannot fail: every
/// value comes from the preregistered M001 constants.
pub fn all_variant_selections() -> Vec<VariantSelection> {
    let mut selections = Vec::with_capacity(180);
    for fusion in FUSION_VARIANTS {
        for alpha in FUSION_ALPHAS {
            for rrf_k in RRF_KS {
                for pooling in POOLING_VARIANTS {
                    for rerank_n in RERANK_CANDIDATE_NS {
                        selections.push(
                            VariantSelection::new(fusion, alpha, rrf_k, pooling, rerank_n)
                                .expect("preregistered grid value"),
                        );
                    }
                }
            }
        }
    }
    selections
}

/// The 9 distinct coarse orderings per pooling: 5 weighted-union
/// alphas at the M004 RRF constant, 3 RRF constants at alpha 0.5, and
/// max at both defaults. Returned as (fusion, alpha, rrf_k).
pub fn fusion_settings() -> Vec<(String, f64, f64)> {
    let mut settings = Vec::with_capacity(9);
    for alpha in FUSION_ALPHAS {
        settings.push(("weighted-union".to_string(), alpha, 60.0));
    }
    for rrf_k in RRF_KS {
        settings.push(("rrf".to_string(), 0.5, rrf_k));
    }
    settings.push(("max".to_string(), 0.5, 60.0));
    settings
}

/// Short id for a fusion-setting under a pooling (no rerank axis).
pub fn setting_id(fusion: &str, alpha: f64, rrf_k: f64, pooling: &str) -> String {
    format!("{fusion}-a{alpha}-k{rrf_k}-p{pooling}")
}

/// Canonical grid member measuring a setting: inert axes take
/// preregistered defaults (alpha 0.5 outside weighted-union, RRF 60.0
/// outside rrf), so the member is a real grid point whose ordering the
/// whole equivalence class shares by construction.
pub fn setting_canonical(
    fusion: &str,
    alpha: f64,
    rrf_k: f64,
    pooling: &str,
    rerank_n: usize,
) -> Result<VariantSelection> {
    let (alpha, rrf_k) = match fusion {
        "weighted-union" => (alpha, 60.0),
        "rrf" => (0.5, rrf_k),
        "max" => (0.5, 60.0),
        other => return Err(anyhow!("unpreregistered fusion variant {other}")),
    };
    VariantSelection::new(fusion, alpha, rrf_k, pooling, rerank_n)
}

/// True exactly for canonical members: the (alpha, rrf_k) pair a
/// setting measures directly.
pub fn is_setting_canonical(selection: &VariantSelection) -> bool {
    match selection.fusion.as_str() {
        "weighted-union" => selection.rrf_k == 60.0,
        "rrf" => selection.alpha == 0.5,
        "max" => selection.alpha == 0.5 && selection.rrf_k == 60.0,
        _ => false,
    }
}

/// Every (alpha, rrf_k) pair sharing a setting's ordering: the inert
/// axis varies over its full preregistered grid. Across the 9 settings
/// this partitions the 180 grid modes exactly (5x3 + 3x5 + 1x15 = 45
/// pairs x 2 poolings x 2 pool sizes).
pub fn setting_members(fusion: &str, alpha: f64, rrf_k: f64) -> Vec<(f64, f64)> {
    match fusion {
        "weighted-union" => RRF_KS.iter().map(|r| (alpha, *r)).collect(),
        "rrf" => FUSION_ALPHAS.iter().map(|a| (*a, rrf_k)).collect(),
        "max" => FUSION_ALPHAS
            .iter()
            .flat_map(|a| RRF_KS.iter().map(|r| (*a, *r)))
            .collect(),
        _ => Vec::new(),
    }
}

/// One measured R001 frontier point: a grid mode at one universe and
/// shortlist K. Coarse records are direct per-(case, variant)
/// measurements; reranked records fan out from their distinct arm
/// (see `rerank_arm`) with identical values by determinism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001FrontierPoint {
    pub schema_version: u16,
    pub mode: String,
    pub fusion: String,
    pub alpha: f64,
    pub rrf_k: f64,
    pub pooling: String,
    pub rerank_n: usize,
    pub reranked: bool,
    /// Distinct-arm id for fanned-out rerank records (`None` for
    /// coarse records, which are all direct measurements).
    pub rerank_arm: Option<String>,
    pub universe_size: usize,
    pub k: usize,
    pub cases: usize,
    pub eligible_relevant: usize,
    pub recovered_relevant: usize,
    pub recall: f64,
    pub mean_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub max_latency_ms: u128,
    pub mean_rerank_ms_per_case: f64,
    pub mean_forwards_per_case: f64,
    pub fallback_cases: usize,
    pub cache_entries: usize,
}

/// One measured distinct re-rank arm: a fusion-setting under one
/// pooling and pool size at one universe, with honest chunk/forward
/// accounting summed over cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001RerankArm {
    pub arm: String,
    pub universe_size: usize,
    pub fusion: String,
    pub alpha: f64,
    pub rrf_k: f64,
    pub pooling: String,
    pub pool_n: usize,
    pub cases: usize,
    pub mean_rerank_ms_per_case: f64,
    pub mean_forwards_per_case: f64,
    pub total_chunks: usize,
    pub total_forwards: usize,
    pub total_dropped: usize,
    pub member_modes: Vec<String>,
}

/// Fingerprint-bound checkpoint for one retrieval universe (M004
/// pattern: resume only on fingerprint and universe match).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001UniverseCheckpoint {
    pub sweep_fingerprint: String,
    pub universe_size: usize,
    pub fixture_fingerprint: String,
    pub points: Vec<R001FrontierPoint>,
    pub arms: Vec<R001RerankArm>,
    pub authority_violations: usize,
}

/// Frozen R001 operating-point selection: at most one of these exists
/// per sweep, primary (K<=32) preferred over extended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001Selection {
    pub mode: String,
    pub fusion: String,
    pub alpha: f64,
    pub rrf_k: f64,
    pub pooling: String,
    pub rerank_n: usize,
    pub k: usize,
    pub extended: bool,
    pub reranked: bool,
    pub recall_64: f64,
    pub recall_128: f64,
    pub recall_256: f64,
    pub mean_latency_ms: f64,
    pub max_latency_ms: u128,
    pub authority_violations: usize,
    pub budget_evidence: BudgetEvidence,
}

/// Residual miss attribution at the frozen point, per universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001Attribution {
    pub universe_size: usize,
    pub attribution: MissAttribution,
}

/// Promotion separator evidence re-attached at the frozen point: the
/// unchanged M004 threshold machinery run on the frozen ranker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001PromotionEvidence {
    pub outcome: PromotionOutcome,
    pub order: PromotionOrderEvidence,
}

/// Sweep resource accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001ResourceEvidence {
    pub cold_load_s: f64,
    pub encoder_weights_mib: u64,
    pub sweep_wallclock_s: u64,
    pub coarse_seconds: f64,
    pub rerank_seconds: f64,
    pub promotion_seconds: f64,
    pub universes_measured: Vec<usize>,
    pub universes_resumed: Vec<usize>,
}

/// Frozen R001 operating-point receipt (positive close only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001FrozenPoint {
    pub protocol: String,
    pub sweep_fingerprint: String,
    pub fixture_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub implementation_commit: String,
    pub selection: R001Selection,
    pub promotion: R001PromotionEvidence,
    pub attribution: Vec<R001Attribution>,
}

/// Full M003 sweep report: frontier tables plus at most one frozen
/// point, or a plan-literal negative summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R001SweepReport {
    pub schema_version: u16,
    pub protocol: String,
    pub sweep_fingerprint: String,
    pub fixture_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub dataset_fingerprint: String,
    pub implementation_commit: String,
    pub cases_per_universe: usize,
    pub eligible_relevant_per_universe: usize,
    pub points: Vec<R001FrontierPoint>,
    pub arms: Vec<R001RerankArm>,
    pub selection: Option<R001Selection>,
    pub negative_summary: Option<String>,
    pub promotion: Option<R001PromotionEvidence>,
    pub attribution: Vec<R001Attribution>,
    pub resources: R001ResourceEvidence,
}

/// Feasible R001 candidate before evidence attachment: the mode clears
/// the gates at one K across all three universes.
#[derive(Debug, Clone)]
pub struct R001Candidate {
    pub mode: String,
    pub fusion: String,
    pub alpha: f64,
    pub rrf_k: f64,
    pub pooling: String,
    pub rerank_n: usize,
    pub k: usize,
    pub reranked: bool,
    pub recall_64: f64,
    pub recall_128: f64,
    pub recall_256: f64,
    pub mean_latency_ms: f64,
    pub max_latency_ms: u128,
}

fn r001_point_lookup<'a>(
    points: &'a [R001FrontierPoint],
    mode: &str,
    k: usize,
) -> Option<&'a R001FrontierPoint> {
    points
        .iter()
        .find(|point| point.mode == mode && point.k == k)
}

/// Feasible candidates over `ks`, ordered smallest/cheapest: K first,
/// then the 256-universe mean latency, then mode name
/// (deterministic). Modes missing any universe/K are skipped, never
/// defaulted.
fn r001_feasible_candidates(
    points_64: &[R001FrontierPoint],
    points_128: &[R001FrontierPoint],
    points_256: &[R001FrontierPoint],
    ks: &[usize],
) -> Vec<R001Candidate> {
    let mut modes = BTreeSet::new();
    for point in points_64 {
        modes.insert(point.mode.clone());
    }
    let mut feasible = Vec::new();
    for mode in modes {
        for k in ks {
            let (Some(point_64), Some(point_128), Some(point_256)) = (
                r001_point_lookup(points_64, &mode, *k),
                r001_point_lookup(points_128, &mode, *k),
                r001_point_lookup(points_256, &mode, *k),
            ) else {
                continue;
            };
            if point_64.recall >= GATE_RECALL_64
                && point_128.recall >= GATE_RECALL_128
                && point_256.recall >= GATE_RECALL_256
            {
                feasible.push(R001Candidate {
                    mode: mode.clone(),
                    fusion: point_256.fusion.clone(),
                    alpha: point_256.alpha,
                    rrf_k: point_256.rrf_k,
                    pooling: point_256.pooling.clone(),
                    rerank_n: point_256.rerank_n,
                    k: *k,
                    reranked: point_256.reranked,
                    recall_64: point_64.recall,
                    recall_128: point_128.recall,
                    recall_256: point_256.recall,
                    mean_latency_ms: point_256.mean_latency_ms,
                    max_latency_ms: point_256.max_latency_ms,
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
}

/// Primary R001 selection: smallest/cheapest clearing point at K<=32
/// with zero authority violations. Budgets are recorded as evidence,
/// not gated, on the primary branch.
pub fn select_r001_primary(
    points_64: &[R001FrontierPoint],
    points_128: &[R001FrontierPoint],
    points_256: &[R001FrontierPoint],
    authority_violations: usize,
) -> Result<R001Candidate> {
    if authority_violations > 0 {
        return Err(anyhow!(
            "r001 frontier has {authority_violations} authority violations"
        ));
    }
    r001_feasible_candidates(points_64, points_128, points_256, &PREREG_PRIMARY_KS)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no r001 operating point clears 0.99/0.98/0.95 at K<=32"))
}

/// Aggregated latency for one (mode, K) across universes: worst p95,
/// worst max, worst mean re-rank cost. Conservative by construction.
#[derive(Debug, Clone)]
pub struct R001CandidateLatency {
    pub p95_ms: f64,
    pub max_ms: u128,
    pub mean_rerank_ms: f64,
}

/// Aggregate worst-case latencies per (mode, K) over the measured
/// universes for extended-K budget evidence.
pub fn aggregate_candidate_latencies(
    points: &[R001FrontierPoint],
) -> BTreeMap<(String, usize), R001CandidateLatency> {
    let mut aggregated: BTreeMap<(String, usize), R001CandidateLatency> = BTreeMap::new();
    for point in points {
        let entry =
            aggregated
                .entry((point.mode.clone(), point.k))
                .or_insert(R001CandidateLatency {
                    p95_ms: 0.0,
                    max_ms: 0,
                    mean_rerank_ms: 0.0,
                });
        entry.p95_ms = entry.p95_ms.max(point.p95_latency_ms);
        entry.max_ms = entry.max_ms.max(point.max_latency_ms);
        entry.mean_rerank_ms = entry.mean_rerank_ms.max(point.mean_rerank_ms_per_case);
    }
    aggregated
}

/// Budget evidence for one (mode, K) from aggregated measured
/// latencies plus run-level costs.
pub fn evidence_for_candidate(
    latency: &R001CandidateLatency,
    cold_load_s: f64,
    weights_mib: u64,
    wallclock_s: u64,
    schema_p95: usize,
) -> BudgetEvidence {
    BudgetEvidence {
        retrieval_p95_ms: latency.p95_ms,
        retrieval_max_ms: latency.max_ms,
        rerank_per_case_ms: latency.mean_rerank_ms,
        cold_load_p95_s: cold_load_s,
        encoder_weights_mib: weights_mib,
        sweep_wallclock_s: wallclock_s,
        schema_p95_bytes: schema_p95,
    }
}

/// Extended-K R001 selection: gate-clearing candidates at K=48/64
/// admitted only with every budget met. Returns the winner with the
/// evidence that admitted it. Primary selection must be attempted
/// first by the driver; this function never promotes an extended
/// point over a feasible primary one.
#[allow(clippy::too_many_arguments)]
pub fn select_r001_extended(
    points_64: &[R001FrontierPoint],
    points_128: &[R001FrontierPoint],
    points_256: &[R001FrontierPoint],
    authority_violations: usize,
    latencies: &BTreeMap<(String, usize), R001CandidateLatency>,
    cold_load_s: f64,
    weights_mib: u64,
    wallclock_s: u64,
    schema_p95: usize,
    limits: &BudgetLimits,
) -> Result<(R001Candidate, BudgetEvidence)> {
    if authority_violations > 0 {
        return Err(anyhow!(
            "r001 frontier has {authority_violations} authority violations"
        ));
    }
    for candidate in
        r001_feasible_candidates(points_64, points_128, points_256, &PREREG_EXTENDED_KS)
    {
        let Some(latency) = latencies.get(&(candidate.mode.clone(), candidate.k)) else {
            continue;
        };
        let evidence =
            evidence_for_candidate(latency, cold_load_s, weights_mib, wallclock_s, schema_p95);
        if extended_selection_allowed(
            candidate.recall_64,
            candidate.recall_128,
            candidate.recall_256,
            &evidence,
            limits,
        )
        .is_ok()
        {
            return Ok((candidate, evidence));
        }
    }
    Err(anyhow!(
        "no r001 extended-K point clears 0.99/0.98/0.95 with all budgets met"
    ))
}

fn percentile_of(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[(quantile * (sorted.len() as f64 - 1.0)).round() as usize % sorted.len()]
}

fn dir_size_bytes(path: &Path) -> Result<u64> {
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            total += dir_size_bytes(&entry.path())?;
        } else {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

/// Implementation commit SHA for sweep receipts. Resolved once at
/// sweep start so a provenance failure fails fast, never after hours
/// of measurement.
fn resolve_implementation_commit(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .context("resolve implementation commit")?;
    if !output.status.success() {
        return Err(anyhow!("git rev-parse HEAD failed"));
    }
    let sha = String::from_utf8(output.stdout)
        .context("implementation commit is not UTF-8")?
        .trim()
        .to_string();
    if sha.is_empty() {
        return Err(anyhow!("empty implementation commit"));
    }
    Ok(sha)
}

/// Sweep fingerprint over preregistration, live fixtures, and
/// implementation commit. Checkpoints bind this value: any input or
/// code change recomputes instead of resuming stale receipts.
fn r001_sweep_fingerprint(
    prereg_fp: &str,
    fixture_fp: &str,
    implementation_commit: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"r001-sweep-v1\n");
    hasher.update(prereg_fp.as_bytes());
    hasher.update(b"\n");
    hasher.update(fixture_fp.as_bytes());
    hasher.update(b"\n");
    hasher.update(implementation_commit.as_bytes());
    hex::encode(hasher.finalize())
}

/// Measure one universe: all 180 coarse grid modes directly per case,
/// then the 36 distinct re-rank arms fanned out to member modes.
/// Returns frontier points, distinct arm measurements, and the
/// authority violation count (which must be zero at selection).
#[allow(clippy::too_many_arguments)]
fn measure_universe<S: SemanticScorer>(
    retrievers: &mut BTreeMap<String, VariantRetriever<S>>,
    ranker_scorer: &FrozenRankerScorer<'_>,
    dev_cases: &[ToolAdvisorCase],
    dataset_fp: &str,
    universe_size: usize,
) -> Result<(Vec<R001FrontierPoint>, Vec<R001RerankArm>, usize)> {
    let fixture = expand_universe(dev_cases, universe_size)?;
    if fixture.len() != M004_DEV_CASES {
        return Err(anyhow!(
            "universe {universe_size} has {} cases; expected {M004_DEV_CASES}",
            fixture.len()
        ));
    }
    let mut expected_per_case: Vec<BTreeSet<String>> = Vec::with_capacity(fixture.len());
    let mut eligible_total = 0usize;
    for case in &fixture {
        let allowed = eligible_deferred_names(case);
        let expected: BTreeSet<String> = case
            .relevance
            .keys()
            .filter(|name| allowed.contains(*name))
            .cloned()
            .collect();
        eligible_total += expected.len();
        expected_per_case.push(expected);
    }
    if eligible_total != M004_ELIGIBLE_RELEVANT {
        return Err(anyhow!(
            "universe {universe_size} has {eligible_total} eligible relevant tools; \
             expected {M004_ELIGIBLE_RELEVANT}"
        ));
    }
    let mut points = Vec::new();
    let mut violations = 0usize;
    // Canonical coarse orderings and timings for the re-rank pools,
    // keyed by setting id: the N=48 grid member is measured first in
    // grid order and N is inert for coarse retrieval.
    let mut canonical_names: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    let mut canonical_ms: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let coarse_started = Instant::now();
    for selection in all_variant_selections() {
        let retriever = retrievers
            .get_mut(selection.pooling.as_str())
            .ok_or_else(|| anyhow!("no retriever for pooling {}", selection.pooling))?;
        let surface = format!(
            "r001-sweep:{dataset_fp}:{}:{universe_size}",
            selection.pooling
        );
        let mut latencies_ms: Vec<f64> = Vec::with_capacity(fixture.len());
        let mut orderings: Vec<Vec<String>> = Vec::with_capacity(fixture.len());
        let mut fallbacks = 0usize;
        let mut cache_entries = 0usize;
        for case in &fixture {
            let result = retriever.retrieve(case, &surface, &selection, SWEEP_FULL_K)?;
            let allowed = eligible_deferred_names(case);
            for name in &result.names {
                if !allowed.contains(name) {
                    violations += 1;
                }
            }
            if result.fallback_to_bm25 {
                fallbacks += 1;
            }
            cache_entries = cache_entries.max(result.cache_entries);
            latencies_ms.push(result.retrieval_ms as f64);
            orderings.push(result.names);
        }
        if fallbacks > 0 {
            return Err(anyhow!(
                "variant {} fell back to BM25 on {fallbacks} cases; measurement invalid",
                selection.point_mode()
            ));
        }
        let mut sorted_latencies = latencies_ms.clone();
        sorted_latencies
            .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        let mean_ms = sorted_latencies.iter().sum::<f64>() / sorted_latencies.len() as f64;
        let p95_ms = percentile_of(&sorted_latencies, 0.95);
        let max_ms = sorted_latencies.last().copied().unwrap_or(0.0) as u128;
        for k in SWEEP_KS {
            let mut recovered = 0usize;
            for (ordering, expected) in orderings.iter().zip(expected_per_case.iter()) {
                let top: BTreeSet<&String> = ordering.iter().take(k.min(ordering.len())).collect();
                recovered += expected.iter().filter(|name| top.contains(name)).count();
            }
            points.push(R001FrontierPoint {
                schema_version: RETRIEVAL_SCHEMA_VERSION,
                mode: selection.point_mode(),
                fusion: selection.fusion.clone(),
                alpha: selection.alpha,
                rrf_k: selection.rrf_k,
                pooling: selection.pooling.clone(),
                rerank_n: selection.rerank_n,
                reranked: false,
                rerank_arm: None,
                universe_size,
                k,
                cases: fixture.len(),
                eligible_relevant: eligible_total,
                recovered_relevant: recovered,
                recall: recovered as f64 / eligible_total as f64,
                mean_latency_ms: mean_ms,
                p95_latency_ms: p95_ms,
                max_latency_ms: max_ms,
                mean_rerank_ms_per_case: 0.0,
                mean_forwards_per_case: 0.0,
                fallback_cases: 0,
                cache_entries,
            });
        }
        if is_setting_canonical(&selection)
            && !canonical_names.contains_key(&setting_id(
                &selection.fusion,
                selection.alpha,
                selection.rrf_k,
                &selection.pooling,
            ))
        {
            canonical_names.insert(
                setting_id(
                    &selection.fusion,
                    selection.alpha,
                    selection.rrf_k,
                    &selection.pooling,
                ),
                orderings,
            );
            canonical_ms.insert(
                setting_id(
                    &selection.fusion,
                    selection.alpha,
                    selection.rrf_k,
                    &selection.pooling,
                ),
                latencies_ms,
            );
        }
    }
    let coarse_seconds = coarse_started.elapsed().as_secs_f64();
    eprintln!(
        "r001 universe {universe_size}: coarse grid measured in {coarse_seconds:.0}s \
         ({} points, {violations} violations)",
        points.len(),
    );
    // Distinct re-rank arms fanned out to member modes.
    let rerank_started = Instant::now();
    let mut arms = Vec::new();
    for (fusion, alpha, rrf_k) in fusion_settings() {
        for pooling in POOLING_VARIANTS {
            let key = setting_id(&fusion, alpha, rrf_k, pooling);
            let Some(coarse_orderings) = canonical_names.get(&key) else {
                return Err(anyhow!("missing canonical ordering for setting {key}"));
            };
            let Some(coarse_ms) = canonical_ms.get(&key) else {
                return Err(anyhow!("missing canonical timings for setting {key}"));
            };
            for pool_n in RERANK_CANDIDATE_NS {
                let arm = format!("{key}-n{pool_n}-u{universe_size}");
                let mut rerank_ms: Vec<f64> = Vec::with_capacity(fixture.len());
                let mut promoted_per_case: Vec<Vec<RankedCandidate>> =
                    Vec::with_capacity(fixture.len());
                let mut total_chunks = 0usize;
                let mut total_forwards = 0usize;
                let mut total_dropped = 0usize;
                for ((case, ordering), case_ms) in fixture
                    .iter()
                    .zip(coarse_orderings.iter())
                    .zip(coarse_ms.iter())
                {
                    let pool: Vec<String> = ordering
                        .iter()
                        .take(pool_n.min(ordering.len()))
                        .cloned()
                        .collect();
                    let report = rerank_pool(ranker_scorer, case, &pool, SWEEP_FULL_K)?;
                    total_chunks += report.chunks;
                    total_forwards += report.forwards;
                    total_dropped += report.dropped;
                    rerank_ms.push(case_ms + report.elapsed_ms as f64);
                    promoted_per_case.push(report.promoted);
                }
                let mut sorted_rerank = rerank_ms.clone();
                sorted_rerank.sort_by(|left, right| {
                    left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
                });
                let mean_ms = sorted_rerank.iter().sum::<f64>() / sorted_rerank.len() as f64;
                let mean_rerank_only = (sorted_rerank.iter().sum::<f64>()
                    - coarse_ms.iter().sum::<f64>())
                    / sorted_rerank.len() as f64;
                let mean_forwards = total_forwards as f64 / fixture.len() as f64;
                let mut member_modes = Vec::new();
                for (member_alpha, member_rrf) in setting_members(&fusion, alpha, rrf_k) {
                    let member =
                        VariantSelection::new(&fusion, member_alpha, member_rrf, pooling, pool_n)?;
                    member_modes.push(member.point_mode());
                    for k in SWEEP_KS {
                        let mut recovered = 0usize;
                        for (promoted, expected) in
                            promoted_per_case.iter().zip(expected_per_case.iter())
                        {
                            let top: BTreeSet<&String> = promoted
                                .iter()
                                .take(k.min(promoted.len()))
                                .map(|entry| &entry.name)
                                .collect();
                            recovered += expected.iter().filter(|name| top.contains(name)).count();
                        }
                        points.push(R001FrontierPoint {
                            schema_version: RETRIEVAL_SCHEMA_VERSION,
                            mode: member.point_mode(),
                            fusion: member.fusion.clone(),
                            alpha: member.alpha,
                            rrf_k: member.rrf_k,
                            pooling: member.pooling.clone(),
                            rerank_n: member.rerank_n,
                            reranked: true,
                            rerank_arm: Some(arm.clone()),
                            universe_size,
                            k,
                            cases: fixture.len(),
                            eligible_relevant: eligible_total,
                            recovered_relevant: recovered,
                            recall: recovered as f64 / eligible_total as f64,
                            mean_latency_ms: mean_ms,
                            p95_latency_ms: percentile_of(&sorted_rerank, 0.95),
                            max_latency_ms: sorted_rerank.last().copied().unwrap_or(0.0) as u128,
                            mean_rerank_ms_per_case: mean_rerank_only,
                            mean_forwards_per_case: mean_forwards,
                            fallback_cases: 0,
                            cache_entries: 0,
                        });
                    }
                }
                member_modes.sort();
                arms.push(R001RerankArm {
                    arm: arm.clone(),
                    universe_size,
                    fusion: fusion.clone(),
                    alpha,
                    rrf_k,
                    pooling: pooling.to_string(),
                    pool_n,
                    cases: fixture.len(),
                    mean_rerank_ms_per_case: mean_rerank_only,
                    mean_forwards_per_case: mean_forwards,
                    total_chunks,
                    total_forwards,
                    total_dropped,
                    member_modes,
                });
            }
        }
    }
    eprintln!(
        "r001 universe {universe_size}: {} re-rank arms measured in {:.0}s",
        arms.len(),
        rerank_started.elapsed().as_secs_f64()
    );
    Ok((points, arms, violations))
}

/// Residual miss attribution for the frozen point: per-tool ranks in
/// the lexical and semantic-only orderings plus the fused rank and
/// margin at the frozen K, per universe. The semantic ordering is the
/// alpha-1.0 weighted-union under the winner's pooling (proven equal
/// to the M004 semantic ordering in M002); the lexical ordering is
/// catalog BM25 by delegation.
#[allow(clippy::too_many_arguments)]
fn attribute_winner<S: SemanticScorer>(
    retrievers: &mut BTreeMap<String, VariantRetriever<S>>,
    ranker_scorer: &FrozenRankerScorer<'_>,
    dev_cases: &[ToolAdvisorCase],
    dataset_fp: &str,
    winner: &R001Selection,
    universe_size: usize,
) -> Result<Vec<R001Attribution>> {
    let fixture = expand_universe(dev_cases, universe_size)?;
    let surface = format!(
        "r001-attribution:{}:{}:{universe_size}",
        dataset_fp, winner.pooling
    );
    let retriever = retrievers
        .get_mut(winner.pooling.as_str())
        .ok_or_else(|| anyhow!("no retriever for pooling {}", winner.pooling))?;
    let semantic_selection =
        VariantSelection::new("weighted-union", 1.0, 60.0, &winner.pooling, 48)?;
    let winner_selection = VariantSelection::new(
        &winner.fusion,
        winner.alpha,
        winner.rrf_k,
        &winner.pooling,
        winner.rerank_n,
    )?;
    let mut attributed = Vec::new();
    for case in &fixture {
        let allowed = eligible_deferred_names(case);
        let relevant: BTreeSet<String> = case
            .relevance
            .keys()
            .filter(|name| allowed.contains(*name))
            .cloned()
            .collect();
        if relevant.is_empty() {
            continue;
        }
        let bm25: Vec<RankedCandidate> = advisor_lexical_ordering(case);
        let semantic_result =
            retriever.retrieve(case, &surface, &semantic_selection, universe_size)?;
        if semantic_result.fallback_to_bm25 {
            return Err(anyhow!(
                "semantic endpoint fell back during attribution for {}",
                case.case_id
            ));
        }
        let semantic: Vec<RankedCandidate> = semantic_result
            .names
            .into_iter()
            .zip(semantic_result.scores)
            .map(|(name, score)| RankedCandidate { name, score })
            .collect();
        let fused: Vec<RankedCandidate> = if winner.reranked {
            let coarse = retriever.retrieve(case, &surface, &winner_selection, universe_size)?;
            let pool: Vec<String> = coarse.names.into_iter().take(winner.rerank_n).collect();
            rerank_pool(ranker_scorer, case, &pool, pool.len())?.promoted
        } else {
            let result = retriever.retrieve(case, &surface, &winner_selection, universe_size)?;
            result
                .names
                .into_iter()
                .zip(result.scores)
                .map(|(name, score)| RankedCandidate { name, score })
                .collect()
        };
        for miss in attribute_misses(&relevant, &bm25, Some(&semantic), &fused, winner.k) {
            attributed.push(R001Attribution {
                universe_size,
                attribution: miss,
            });
        }
    }
    Ok(attributed)
}

/// Run the preregistered M003 sweep once: verify fixtures live, load
/// the frozen encoder and ranker, measure every universe (resuming
/// fingerprint-bound checkpoints), select at most one operating point,
/// and re-attach the unchanged M004 promotion separator at the frozen
/// point. Returns the full report; a negative verdict is data
/// (`selection: None` with `negative_summary`), and the caller decides
/// whether to fail on it after receipts are written.
pub fn run_r001_sweep(output_dir: &Path) -> Result<R001SweepReport> {
    let sweep_started = Instant::now();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let prereg = preregistration(EXPECTED_FIXTURE_FINGERPRINT);
    let prereg_fp = prereg_fingerprint(&prereg)?;
    let live_fixture_fp = fixture_fingerprint()?;
    verify_fixture_fingerprint(&live_fixture_fp, EXPECTED_FIXTURE_FINGERPRINT)?;
    let dev_fp = dev_partition_tripwire()?;
    let implementation_commit = resolve_implementation_commit(&root)?;
    let sweep_fp = r001_sweep_fingerprint(&prereg_fp, &live_fixture_fp, &implementation_commit);
    eprintln!("r001 sweep fingerprint {sweep_fp} (commit {implementation_commit})");

    let cases = load_cases(Some(&root.join(PREREG_DATASET))).context("load frozen corpus")?;
    let dataset_fp = dataset_fingerprint(&cases)?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    if dev_cases.len() != M004_DEV_CASES {
        return Err(anyhow!(
            "dev partition has {} cases; expected {M004_DEV_CASES}",
            dev_cases.len()
        ));
    }

    let device = Device::Cpu;
    let encoder_path = root.join(PREREG_ENCODER_MANIFEST);
    let load_started = Instant::now();
    let encoder = CandleBertSequenceEncoder::load(&encoder_path, &device)?;
    let cold_load_s = load_started.elapsed().as_secs_f64();
    let asset_dir = encoder_path
        .parent()
        .ok_or_else(|| anyhow!("encoder manifest has no parent directory"))?;
    let weights_mib = dir_size_bytes(asset_dir)? / 1024 / 1024;
    eprintln!("r001 encoder loaded in {cold_load_s:.1}s ({weights_mib} MiB weights)");

    let ranker_path = root.join(PREREG_RANKER_ARTIFACT);
    let ranker = load_ranker_artifact(&ranker_path, &device)?;
    if ranker.manifest.dev_partition_fingerprint != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!(
            "ranker dev partition changed: sweep cannot measure the selected artifact"
        ));
    }
    let ranker_scorer = FrozenRankerScorer::new(&ranker)?;
    let abstention_threshold = ranker.manifest.calibration.abstention_threshold;
    let temperature = ranker.manifest.calibration.candidate_temperature;
    let bias = ranker.manifest.calibration.candidate_bias;

    // Schema-size evidence over deferred candidates (promotion shape).
    let mut schema_bytes = Vec::with_capacity(dev_cases.len());
    for case in &dev_cases {
        schema_bytes.push(
            case.candidates
                .iter()
                .filter(|candidate| candidate.disclosure == "deferred")
                .map(|candidate| {
                    serde_json::to_vec(candidate)
                        .map(|bytes| bytes.len())
                        .unwrap_or(0)
                })
                .sum::<usize>() as f64,
        );
    }
    schema_bytes
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let schema_p95 = percentile_of(&schema_bytes, 0.95) as usize;

    std::fs::create_dir_all(output_dir)?;
    let mut retrievers: BTreeMap<String, VariantRetriever<EncoderSemanticScorer<'_>>> =
        BTreeMap::new();
    for pooling in POOLING_VARIANTS {
        retrievers.insert(
            pooling.to_string(),
            VariantRetriever::new(EncoderSemanticScorer::new(&encoder)),
        );
    }
    let mut all_points = Vec::new();
    let mut all_arms = Vec::new();
    let mut violations = 0usize;
    let mut universes_measured = Vec::new();
    let mut universes_resumed = Vec::new();
    let mut coarse_seconds = 0.0f64;
    for universe_size in PREREG_UNIVERSES {
        let checkpoint_file = checkpoint_path(&output_dir.display().to_string(), universe_size);
        if let Ok(bytes) = std::fs::read(&checkpoint_file) {
            if let Ok(checkpoint) = serde_json::from_slice::<R001UniverseCheckpoint>(&bytes) {
                if checkpoint_accepts(
                    &checkpoint.sweep_fingerprint,
                    checkpoint.universe_size,
                    &sweep_fp,
                    universe_size,
                ) && checkpoint.fixture_fingerprint == live_fixture_fp
                {
                    eprintln!("r001 universe {universe_size}: resumed from checkpoint");
                    violations += checkpoint.authority_violations;
                    all_points.extend(checkpoint.points);
                    all_arms.extend(checkpoint.arms);
                    universes_resumed.push(universe_size);
                    continue;
                }
            }
        }
        let universe_started = Instant::now();
        let (points, arms, universe_violations) = measure_universe(
            &mut retrievers,
            &ranker_scorer,
            &dev_cases,
            &dataset_fp,
            universe_size,
        )?;
        let universe_seconds = universe_started.elapsed().as_secs_f64();
        coarse_seconds += universe_seconds;
        violations += universe_violations;
        std::fs::write(
            &checkpoint_file,
            serde_json::to_vec_pretty(&R001UniverseCheckpoint {
                sweep_fingerprint: sweep_fp.clone(),
                universe_size,
                fixture_fingerprint: live_fixture_fp.clone(),
                points: points.clone(),
                arms: arms.clone(),
                authority_violations: universe_violations,
            })?,
        )?;
        all_points.extend(points);
        all_arms.extend(arms);
        universes_measured.push(universe_size);
    }
    // Coarse vs re-rank split is approximated from arm cost reports;
    // the wall-clock total is exact.
    let rerank_seconds: f64 = all_arms
        .iter()
        .map(|arm| arm.mean_rerank_ms_per_case * arm.cases as f64 / 1000.0)
        .sum();
    coarse_seconds -= rerank_seconds;

    let wallclock_s = sweep_started.elapsed().as_secs();
    let points_64: Vec<R001FrontierPoint> = all_points
        .iter()
        .filter(|point| point.universe_size == 64)
        .cloned()
        .collect();
    let points_128: Vec<R001FrontierPoint> = all_points
        .iter()
        .filter(|point| point.universe_size == 128)
        .cloned()
        .collect();
    let points_256: Vec<R001FrontierPoint> = all_points
        .iter()
        .filter(|point| point.universe_size == 256)
        .cloned()
        .collect();

    // Selection: primary first, then bounded extended, else negative.
    let mut selection: Option<R001Selection> = None;
    let mut negative_summary: Option<String> = None;
    match select_r001_primary(&points_64, &points_128, &points_256, violations) {
        Ok(candidate) => {
            let latencies = aggregate_candidate_latencies(&all_points);
            let latency = latencies
                .get(&(candidate.mode.clone(), candidate.k))
                .cloned()
                .unwrap_or(R001CandidateLatency {
                    p95_ms: 0.0,
                    max_ms: 0,
                    mean_rerank_ms: 0.0,
                });
            selection = Some(R001Selection {
                mode: candidate.mode.clone(),
                fusion: candidate.fusion.clone(),
                alpha: candidate.alpha,
                rrf_k: candidate.rrf_k,
                pooling: candidate.pooling.clone(),
                rerank_n: candidate.rerank_n,
                k: candidate.k,
                extended: false,
                reranked: candidate.reranked,
                recall_64: candidate.recall_64,
                recall_128: candidate.recall_128,
                recall_256: candidate.recall_256,
                mean_latency_ms: candidate.mean_latency_ms,
                max_latency_ms: candidate.max_latency_ms,
                authority_violations: violations,
                budget_evidence: evidence_for_candidate(
                    &latency,
                    cold_load_s,
                    weights_mib,
                    wallclock_s,
                    schema_p95,
                ),
            });
        }
        Err(primary_error) => {
            let latencies = aggregate_candidate_latencies(&all_points);
            match select_r001_extended(
                &points_64,
                &points_128,
                &points_256,
                violations,
                &latencies,
                cold_load_s,
                weights_mib,
                wallclock_s,
                schema_p95,
                &prereg.budgets,
            ) {
                Ok((candidate, evidence)) => {
                    selection = Some(R001Selection {
                        mode: candidate.mode.clone(),
                        fusion: candidate.fusion.clone(),
                        alpha: candidate.alpha,
                        rrf_k: candidate.rrf_k,
                        pooling: candidate.pooling.clone(),
                        rerank_n: candidate.rerank_n,
                        k: candidate.k,
                        extended: true,
                        reranked: candidate.reranked,
                        recall_64: candidate.recall_64,
                        recall_128: candidate.recall_128,
                        recall_256: candidate.recall_256,
                        mean_latency_ms: candidate.mean_latency_ms,
                        max_latency_ms: candidate.max_latency_ms,
                        authority_violations: violations,
                        budget_evidence: evidence,
                    });
                }
                Err(extended_error) => {
                    negative_summary = Some(format!(
                        "no r001 operating point clears 0.99/0.98/0.95 \
                         (primary: {primary_error}; extended: {extended_error}; \
                         best recalls 64/128/256={:.4}/{:.4}/{:.4}; violations={violations}; \
                         cold-load={cold_load_s:.1}s weights={weights_mib}MiB \
                         wallclock={wallclock_s}s)",
                        best_recall(&points_64),
                        best_recall(&points_128),
                        best_recall(&points_256),
                    ));
                }
            }
        }
    }
    // Wall-clock overrun closes negatively on resources even with a
    // clearing point: the sweep did not complete inside its budget.
    if wallclock_s > BUDGET_SWEEP_WALLCLOCK_S {
        negative_summary = Some(format!(
            "r001 sweep exceeded the {BUDGET_SWEEP_WALLCLOCK_S}s wall-clock budget \
             ({wallclock_s}s elapsed); selection invalidated on resources"
        ));
        selection = None;
    }

    // Positive path only: attribution plus the unchanged promotion
    // separator at the frozen point.
    let mut promotion: Option<R001PromotionEvidence> = None;
    let mut attribution = Vec::new();
    let mut promotion_seconds = 0.0f64;
    if let Some(winner) = selection.clone() {
        let promotion_started = Instant::now();
        for universe_size in PREREG_UNIVERSES {
            attribution.extend(attribute_winner(
                &mut retrievers,
                &ranker_scorer,
                &dev_cases,
                &dataset_fp,
                &winner,
                universe_size,
            )?);
        }
        let views =
            promotion_case_views(&ranker, &dev_cases, abstention_threshold, temperature, bias)?;
        let outcomes: Vec<PromotionOutcome> = promotion_threshold_grid()
            .iter()
            .map(|threshold| promotion_outcome_for_threshold(&views, *threshold))
            .collect();
        match select_promotion_threshold(&outcomes) {
            Ok(outcome) => {
                let order = promotion_order_evidence(
                    &ranker,
                    &dev_cases,
                    PROMOTION_EVIDENCE_SAMPLE_CASES,
                    PROMOTION_EVIDENCE_PERMUTATIONS,
                    PREREG_SEED,
                    abstention_threshold,
                    temperature,
                    bias,
                    outcome.threshold,
                )?;
                promotion_seconds = promotion_started.elapsed().as_secs_f64();
                promotion = Some(R001PromotionEvidence { outcome, order });
            }
            Err(promotion_error) => {
                negative_summary = Some(format!(
                    "r001 retrieval cleared at {} K={} but the promotion separator \
                     failed: {promotion_error}",
                    winner.mode, winner.k
                ));
                selection = None;
                attribution.clear();
            }
        }
    }
    // Promotion borrows end here; receipts below need no scoring.
    let wallclock_total_s = sweep_started.elapsed().as_secs();
    let report = R001SweepReport {
        schema_version: RETRIEVAL_ARCH_SCHEMA_VERSION,
        protocol: RETRIEVAL_ARCH_PROTOCOL.into(),
        sweep_fingerprint: sweep_fp,
        fixture_fingerprint: live_fixture_fp,
        dev_partition_fingerprint: dev_fp,
        dataset_fingerprint: dataset_fp,
        implementation_commit,
        cases_per_universe: M004_DEV_CASES,
        eligible_relevant_per_universe: M004_ELIGIBLE_RELEVANT,
        points: all_points,
        arms: all_arms,
        selection: selection.clone(),
        negative_summary: negative_summary.clone(),
        promotion: promotion.clone(),
        attribution: attribution.clone(),
        resources: R001ResourceEvidence {
            cold_load_s,
            encoder_weights_mib: weights_mib,
            sweep_wallclock_s: wallclock_total_s,
            coarse_seconds,
            rerank_seconds,
            promotion_seconds,
            universes_measured,
            universes_resumed,
        },
    };
    std::fs::write(
        output_dir.join("r001-sweep-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if let (Some(winner), Some(promotion_evidence)) = (selection, promotion) {
        std::fs::write(
            output_dir.join("r001-frozen-point.json"),
            serde_json::to_vec_pretty(&R001FrozenPoint {
                protocol: RETRIEVAL_ARCH_PROTOCOL.into(),
                sweep_fingerprint: report.sweep_fingerprint.clone(),
                fixture_fingerprint: report.fixture_fingerprint.clone(),
                dev_partition_fingerprint: report.dev_partition_fingerprint.clone(),
                implementation_commit: report.implementation_commit.clone(),
                selection: winner,
                promotion: promotion_evidence,
                attribution,
            })?,
        )?;
    }
    eprintln!(
        "r001 sweep done in {wallclock_total_s}s: {}",
        negative_summary.as_deref().unwrap_or("point frozen")
    );
    Ok(report)
}

/// Best recall over any measured point: the negative-verdict ceiling.
fn best_recall(points: &[R001FrontierPoint]) -> f64 {
    points
        .iter()
        .map(|point| point.recall)
        .fold(0.0f64, f64::max)
}

#[cfg(test)]
mod tests {
    use super::super::operating_point::expand_universe;
    use super::super::sequence_retrieval::{select_retrieval_point, RetrievalFrontierPoint};
    use super::super::{ToolAdvisorCandidate, CASE_SCHEMA_VERSION};
    use super::*;
    use std::collections::BTreeMap;

    fn ranked(entries: &[(&str, f64)]) -> Vec<RankedCandidate> {
        entries
            .iter()
            .map(|(name, score)| RankedCandidate {
                name: (*name).into(),
                score: *score,
            })
            .collect()
    }

    fn relevant(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).into()).collect()
    }

    #[test]
    fn dual_signal_miss_attributes_both_ranks_and_margin() {
        let bm25 = ranked(&[("w", 3.0), ("x", 2.0), ("z", 1.0), ("y", 0.0)]);
        let semantic = ranked(&[("w", 0.9), ("x", 0.8), ("y", 0.2), ("z", 0.1)]);
        let fused = ranked(&[("w", 0.9), ("x", 0.8), ("y", 0.2), ("z", 0.1)]);
        let missed = attribute_misses(&relevant(&["y"]), &bm25, Some(&semantic), &fused, 2);
        assert_eq!(missed.len(), 1);
        let miss = &missed[0];
        assert_eq!(miss.tool_name, "y");
        assert_eq!(miss.bm25_rank, Some(4));
        assert_eq!(miss.semantic_rank, Some(3));
        assert_eq!(miss.fused_rank, Some(3));
        assert_eq!(miss.cause, MissCause::DualSignalMiss);
        let margin = miss.margin_to_boundary.expect("margin");
        assert!((margin - (0.2 - 0.8)).abs() < 1e-12);
        assert!(margin < 0.0);
    }

    #[test]
    fn recovered_tools_produce_no_attribution() {
        let bm25 = ranked(&[("w", 3.0), ("y", 2.0)]);
        let semantic = ranked(&[("y", 0.9), ("w", 0.1)]);
        let fused = ranked(&[("y", 0.9), ("w", 0.4)]);
        assert!(
            attribute_misses(&relevant(&["y", "w"]), &bm25, Some(&semantic), &fused, 2).is_empty()
        );
        assert!(attribute_misses(&relevant(&[]), &bm25, Some(&semantic), &fused, 2).is_empty());
    }

    #[test]
    fn single_signal_reach_is_weight_sensitive() {
        // BM25 reaches K=2 but semantic does not; the fusion drops the tool.
        let bm25 = ranked(&[("t", 3.0), ("w", 2.0), ("x", 1.0)]);
        let semantic = ranked(&[("w", 0.9), ("x", 0.8), ("t", 0.1)]);
        let fused = ranked(&[("w", 0.9), ("x", 0.7), ("t", 0.2)]);
        let missed = attribute_misses(&relevant(&["t"]), &bm25, Some(&semantic), &fused, 2);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].cause, MissCause::FusionWeightSensitive);
    }

    #[test]
    fn both_signals_reaching_with_fused_drop_is_margin_miss() {
        let bm25 = ranked(&[("m", 3.0), ("w", 2.0), ("x", 1.0)]);
        let semantic = ranked(&[("w", 0.9), ("m", 0.8), ("x", 0.1)]);
        let fused = ranked(&[("w", 0.9), ("x", 0.7), ("m", 0.6)]);
        let missed = attribute_misses(&relevant(&["m"]), &bm25, Some(&semantic), &fused, 2);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].cause, MissCause::FusionMarginMiss);
    }

    #[test]
    fn bm25_only_orders_classify_lexical_gap() {
        let bm25 = ranked(&[("w", 3.0), ("x", 2.0)]);
        let fused = ranked(&[("w", 0.9), ("x", 0.8)]);
        // Tool absent from BM25 with no semantic ordering: lexical gap.
        let missed = attribute_misses(&relevant(&["y"]), &bm25, None, &fused, 2);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].cause, MissCause::LexicalGap);
        assert_eq!(missed[0].bm25_rank, None);
        assert_eq!(missed[0].margin_to_boundary, None);
        // Tool BM25-reachable but fusion-dropped: margin miss even
        // without the semantic ordering.
        let bm25 = ranked(&[("y", 3.0), ("w", 2.0), ("x", 1.0)]);
        let missed = attribute_misses(&relevant(&["y"]), &bm25, None, &fused, 1);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].cause, MissCause::FusionMarginMiss);
    }

    #[test]
    fn absent_everywhere_is_insufficient_evidence() {
        let empty: Vec<RankedCandidate> = Vec::new();
        let missed = attribute_misses(&relevant(&["ghost"]), &empty, None, &empty, 2);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].cause, MissCause::InsufficientEvidence);
    }

    #[test]
    fn attribution_ranks_are_deterministic_under_ties() {
        let ordering = ranked(&[("b", 1.0), ("a", 1.0), ("c", 1.0)]);
        assert_eq!(rank_of(&ordering, "b"), Some(1));
        assert_eq!(
            attribute_misses(&relevant(&["c"]), &ordering, None, &ordering, 2)[0].fused_rank,
            Some(3)
        );
    }

    #[test]
    fn weighted_union_reference_vectors() {
        let bm25 = vec![("a".to_string(), 10.0), ("b".to_string(), 0.0)];
        let semantic = vec![("a".to_string(), 0.0), ("b".to_string(), 10.0)];
        let mid = fuse_weighted_union(&bm25, &semantic, 0.5);
        assert_eq!(
            mid.iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!((mid[0].score - 0.5).abs() < 1e-12);
        let lexical = fuse_weighted_union(&bm25, &semantic, 0.0);
        assert_eq!(lexical[0].name, "a");
        assert!((lexical[0].score - 1.0).abs() < 1e-12);
        let semantic_only = fuse_weighted_union(&bm25, &semantic, 1.0);
        assert_eq!(semantic_only[0].name, "b");
        assert!((semantic_only[0].score - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rrf_reference_vector_matches_hand_computation() {
        let bm25 = vec![("a".to_string(), 2.0), ("b".to_string(), 1.0)];
        let semantic = vec![("b".to_string(), 0.7)];
        let fused = fuse_rrf_with_k(&bm25, &semantic, 60.0);
        assert_eq!(fused[0].name, "b");
        assert!((fused[0].score - (1.0 / 62.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert!((fused[1].score - 1.0 / 61.0).abs() < 1e-12);
    }

    #[test]
    fn max_fusion_reference_vector() {
        let bm25 = vec![
            ("a".to_string(), 10.0),
            ("b".to_string(), 5.0),
            ("c".to_string(), 0.0),
        ];
        let semantic = vec![
            ("a".to_string(), 0.0),
            ("b".to_string(), 5.0),
            ("c".to_string(), 10.0),
        ];
        let fused = fuse_max(&bm25, &semantic);
        let names: Vec<&str> = fused.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c", "b"]);
        assert!((fused[2].score - 0.5).abs() < 1e-12);
    }

    #[test]
    fn fusion_outputs_are_deterministic() {
        let bm25 = vec![("b".to_string(), 1.0), ("a".to_string(), 1.0)];
        let semantic = vec![("a".to_string(), 0.5), ("b".to_string(), 0.5)];
        for fused in [
            fuse_weighted_union(&bm25, &semantic, 0.5),
            fuse_rrf_with_k(&bm25, &semantic, 60.0),
            fuse_max(&bm25, &semantic),
        ] {
            assert_eq!(fused[0].name, "a");
            assert_eq!(fused[1].name, "b");
        }
    }

    #[test]
    fn preregistration_round_trips_and_fingerprint_excludes_itself() {
        let prereg = preregistration("fixture-fp");
        let bytes = serde_json::to_vec(&prereg).expect("serialize");
        let back: Preregistration = serde_json::from_slice(&bytes).expect("round trip");
        assert_eq!(prereg, back);
        assert_eq!(prereg.protocol, RETRIEVAL_ARCH_PROTOCOL);
        assert_eq!(prereg.schema_version, RETRIEVAL_ARCH_SCHEMA_VERSION);
        assert_eq!(prereg.sweep_command, PREREG_SWEEP_COMMAND);
        let fingerprint = prereg_fingerprint(&prereg).expect("fingerprint");
        assert_eq!(fingerprint.len(), 64);
        let serialized = serde_json::to_string(&prereg).expect("serialize");
        assert!(
            !serialized.contains(&fingerprint),
            "sweep fingerprint must not appear inside the preregistration"
        );
        let mut mutated = prereg.clone();
        mutated.seed ^= 0xFFFF_FFFF_FFFF_FFFF;
        assert_ne!(
            prereg_fingerprint(&mutated).expect("fingerprint"),
            fingerprint
        );
    }

    #[test]
    fn fixture_fingerprint_tracks_input_bytes() {
        let first = fixture_fingerprint_for(b"receipt-v1", "dev-fp");
        assert_eq!(first, fixture_fingerprint_for(b"receipt-v1", "dev-fp"));
        assert_ne!(first, fixture_fingerprint_for(b"receipt-v2", "dev-fp"));
        assert_ne!(
            first,
            fixture_fingerprint_for(b"receipt-v1", "other-dev-fp")
        );
        verify_fixture_fingerprint(&first, &first).expect("matching fingerprint");
        verify_fixture_fingerprint("deadbeef", &first).expect_err("mismatch must fail closed");
    }

    #[test]
    fn live_fixture_fingerprint_is_deterministic() {
        let first = fixture_fingerprint().expect("live fixture fingerprint");
        let second = fixture_fingerprint().expect("live fixture fingerprint");
        assert_eq!(first, second);
        assert_eq!(first, EXPECTED_FIXTURE_FINGERPRINT);
        verify_fixture_fingerprint(&first, EXPECTED_FIXTURE_FINGERPRINT).expect("match");
        eprintln!("live fixture fingerprint: {first}");
    }

    #[test]
    fn dev_partition_tripwire_holds() {
        let dev_fp = dev_partition_tripwire().expect("frozen dev partition");
        assert_eq!(dev_fp, EXPECTED_DEV_PARTITION_FINGERPRINT);
    }

    fn budget_evidence() -> BudgetEvidence {
        BudgetEvidence {
            retrieval_p95_ms: 100.0,
            retrieval_max_ms: 500,
            rerank_per_case_ms: 200.0,
            cold_load_p95_s: 5.0,
            encoder_weights_mib: 64,
            sweep_wallclock_s: 3600,
            schema_p95_bytes: 1024,
        }
    }

    fn budget_limits() -> BudgetLimits {
        preregistration("fp").budgets
    }

    #[test]
    fn budgets_fail_closed_on_any_breach() {
        verify_budgets(&budget_evidence(), &budget_limits()).expect("within budget");
        let mut breached = budget_evidence();
        breached.retrieval_p95_ms = BUDGET_RETRIEVAL_P95_MS + 1.0;
        assert!(verify_budgets(&breached, &budget_limits()).is_err());
        let mut breached = budget_evidence();
        breached.sweep_wallclock_s = BUDGET_SWEEP_WALLCLOCK_S + 1;
        assert!(verify_budgets(&breached, &budget_limits()).is_err());
    }

    #[test]
    fn extended_k_requires_gates_and_budgets() {
        // Both pass: allowed.
        extended_selection_allowed(0.99, 0.98, 0.95, &budget_evidence(), &budget_limits())
            .expect("allowed");
        // Gate miss with passing budgets: refused.
        assert!(
            extended_selection_allowed(0.99, 0.98, 0.94, &budget_evidence(), &budget_limits())
                .is_err()
        );
        // Gates pass with breached budget: refused.
        let mut breached = budget_evidence();
        breached.cold_load_p95_s = BUDGET_COLD_LOAD_P95_S + 1.0;
        assert!(extended_selection_allowed(0.99, 0.98, 0.95, &breached, &budget_limits()).is_err());
    }

    #[test]
    fn checkpoint_binding_resumes_only_on_match() {
        assert!(checkpoint_accepts("fp", 64, "fp", 64));
        assert!(!checkpoint_accepts("other", 64, "fp", 64));
        assert!(!checkpoint_accepts("fp", 128, "fp", 64));
        assert_eq!(
            checkpoint_path("out", 64),
            PathBuf::from("out/r001-checkpoint-frontier-64.json")
        );
    }

    fn identity_point(universe: usize, k: usize, mode: &str) -> RetrievalFrontierPoint {
        RetrievalFrontierPoint {
            mode: mode.into(),
            k,
            cases: 1,
            relevant_tools: 1,
            recovered_tools: 1,
            recall: 1.0,
            mean_latency_ms: 0.0,
            max_latency_ms: 0,
            candidate_universe_size: universe,
            shortlist_k: k,
            eligible_relevant_tools: 1,
            recovered_relevant_tools: 1,
        }
    }

    #[test]
    fn universe_and_shortlist_identity_mismatch_fails_closed() {
        let points = vec![
            identity_point(64, 16, "weighted-union"),
            identity_point(64, 24, "weighted-union"),
        ];
        assert!(select_retrieval_point(&points, 64, 16, "weighted-union").is_ok());
        assert!(select_retrieval_point(&points, 128, 16, "weighted-union").is_err());
        assert!(select_retrieval_point(&points, 64, 32, "weighted-union").is_err());
        assert!(select_retrieval_point(&points, 64, 16, "rrf").is_err());
        let duplicated = vec![
            identity_point(64, 16, "weighted-union"),
            identity_point(64, 16, "weighted-union"),
        ];
        assert!(select_retrieval_point(&duplicated, 64, 16, "weighted-union").is_err());
    }

    fn synthetic_candidate(name: &str, disclosure: &str) -> ToolAdvisorCandidate {
        ToolAdvisorCandidate {
            name: name.into(),
            description: format!("{name} descriptor"),
            category: "ReadOnly".into(),
            disclosure: disclosure.into(),
            synthetic_identity: false,
        }
    }

    fn synthetic_case(id: &str, relevant: &str) -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: id.into(),
            context: format!("synthetic context {id}"),
            candidates: vec![
                synthetic_candidate(relevant, "deferred"),
                synthetic_candidate(&format!("{id}-other"), "deferred"),
                synthetic_candidate(&format!("{id}-immediate"), "immediate"),
            ],
            relevance: BTreeMap::from([(relevant.to_string(), 3)]),
            preferred_order: vec![relevant.into()],
            none: false,
            tags: vec![],
            group_id: id.into(),
            provenance: "r001-test".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn non_deferred_candidates_never_enter_attribution_universes() {
        let case = synthetic_case("case-a", "tool-a");
        let eligible = eligible_deferred_names(&case);
        assert!(eligible.contains("tool-a"));
        assert!(eligible.contains("case-a-other"));
        assert!(!eligible.contains("case-a-immediate"));
        verify_universe(&bm25_ordering(&case), &eligible).expect("deferred-only ordering");
        let outsider = vec![RankedCandidate {
            name: "case-a-immediate".into(),
            score: 9.0,
        }];
        verify_universe(&outsider, &eligible).expect_err("outsider must fail closed");
    }

    #[test]
    fn expansion_preserves_relevant_tools_on_synthetic_corpus() {
        let cases = vec![
            synthetic_case("case-a", "tool-a"),
            synthetic_case("case-b", "tool-b"),
        ];
        let expanded = expand_universe(&cases, 8).expect("expansion");
        assert_eq!(expanded.len(), 2);
        for (source, fixture) in cases.iter().zip(expanded.iter()) {
            assert_eq!(fixture.candidates.len(), 8);
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
        assert_eq!(expanded, expand_universe(&cases, 8).expect("expansion"));
    }

    fn missed_tool_set(cases: &[ToolAdvisorCase], k: usize) -> BTreeSet<String> {
        let mut missed = BTreeSet::new();
        for case in cases {
            let allowed = eligible_deferred_names(case);
            let top: BTreeSet<String> = bm25_ordering(case)
                .into_iter()
                .take(k)
                .map(|entry| entry.name)
                .collect();
            for name in case.relevance.keys() {
                if allowed.contains(name) && !top.contains(name) {
                    missed.insert(format!("{}:{name}", case.case_id));
                }
            }
        }
        missed
    }

    #[test]
    fn bm25_dev_diagnostic_reproduces_flat_m004_ceiling() {
        let cases = load_cases(None).expect("builtin corpus");
        let partition = partition_cases(&cases);
        assert_eq!(partition.dev_cases.len(), M004_DEV_CASES);
        let dev: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        for size in PREREG_UNIVERSES {
            let expanded = expand_universe(&dev, size).expect("expansion");
            let mut miss_sets = Vec::new();
            for k in PREREG_PRIMARY_KS {
                let (recovered, eligible) = bm25_recall_at_k(&expanded, k);
                assert_eq!(
                    (recovered, eligible),
                    (M004_BM25_RECOVERED, M004_ELIGIBLE_RELEVANT),
                    "BM25 must reproduce the M004 ceiling at universe {size} K={k}"
                );
                miss_sets.push(missed_tool_set(&expanded, k));
            }
            // The same tools miss at every K: a systematic lexical gap,
            // not a cutoff edge.
            assert!(
                miss_sets.windows(2).all(|pair| pair[0] == pair[1]),
                "BM25 miss set must be K-invariant at universe {size}"
            );
        }
    }

    // ---- M002 variant tests ----

    use std::cell::RefCell;

    fn variant_selection(
        fusion: &str,
        alpha: f64,
        rrf_k: f64,
        pooling: &str,
        rerank_n: usize,
    ) -> VariantSelection {
        VariantSelection::new(fusion, alpha, rrf_k, pooling, rerank_n).expect("valid selection")
    }

    fn signal(entries: &[(&str, f64)]) -> Vec<(String, f64)> {
        entries
            .iter()
            .map(|(name, score)| ((*name).to_string(), *score))
            .collect()
    }

    #[test]
    fn variant_grid_is_fully_constructible_and_rejects_unknowns() {
        let mut modes = BTreeSet::new();
        for fusion in FUSION_VARIANTS {
            for alpha in FUSION_ALPHAS {
                for rrf_k in RRF_KS {
                    for pooling in POOLING_VARIANTS {
                        for rerank_n in RERANK_CANDIDATE_NS {
                            let selection =
                                variant_selection(fusion, alpha, rrf_k, pooling, rerank_n);
                            assert!(modes.insert(selection.point_mode()));
                        }
                    }
                }
            }
        }
        // Injective over the grid: 3 fusions x 5 alphas x 3 RRF_K x 2
        // poolings x 2 pool sizes, no shared mode string.
        assert_eq!(
            modes.len(),
            FUSION_VARIANTS.len()
                * FUSION_ALPHAS.len()
                * RRF_KS.len()
                * POOLING_VARIANTS.len()
                * RERANK_CANDIDATE_NS.len()
        );
        assert!(VariantSelection::new("tf-idf", 0.5, 60.0, "mean", 48).is_err());
        assert!(VariantSelection::new("rrf", 0.3, 60.0, "mean", 48).is_err());
        assert!(VariantSelection::new("rrf", 0.5, 45.0, "mean", 48).is_err());
        assert!(VariantSelection::new("max", 0.5, 60.0, "median", 48).is_err());
        assert!(VariantSelection::new("max", 0.5, 60.0, "mean", 32).is_err());
        parse_pooling("mean").expect("mean parses");
        parse_pooling("cls").expect("cls parses");
        parse_pooling("median").expect_err("unknown pooling fails closed");
    }

    #[test]
    fn fusion_dispatch_matches_reference_vectors_across_full_grids() {
        let bm25 = signal(&[("a", 3.0), ("b", 2.0), ("c", 0.0), ("d", 1.0)]);
        let semantic = signal(&[("a", 0.1), ("b", 0.9), ("c", 0.7), ("e", 0.5)]);
        for alpha in FUSION_ALPHAS {
            let selection = variant_selection("weighted-union", alpha, 60.0, "mean", 48);
            assert_eq!(
                select_coarse_ordering(&bm25, &semantic, &selection).expect("dispatch"),
                fuse_weighted_union(&bm25, &semantic, alpha),
                "weighted-union must call the reference function at alpha {alpha}"
            );
        }
        for rrf_k in RRF_KS {
            let selection = variant_selection("rrf", 0.5, rrf_k, "mean", 48);
            assert_eq!(
                select_coarse_ordering(&bm25, &semantic, &selection).expect("dispatch"),
                fuse_rrf_with_k(&bm25, &semantic, rrf_k),
                "rrf must call the reference function at K {rrf_k}"
            );
        }
        let selection = variant_selection("max", 0.5, 60.0, "cls", 64);
        assert_eq!(
            select_coarse_ordering(&bm25, &semantic, &selection).expect("dispatch"),
            fuse_max(&bm25, &semantic),
            "max must call the reference function"
        );
        let bogus = VariantSelection {
            fusion: "tf-idf".into(),
            alpha: 0.5,
            rrf_k: 60.0,
            pooling: "mean".into(),
            rerank_n: 48,
        };
        select_coarse_ordering(&bm25, &semantic, &bogus).expect_err("bogus fusion fails closed");
    }

    #[test]
    fn alpha_half_reproduces_equal_weight_ordering() {
        // Hand computation: normalized BM25 is a=1, b=2/3, c=0;
        // normalized semantic is a=0, b=2/3, c=1. Equal weights give
        // b=4/3 ahead of the a=c=1 tie, broken by name.
        let bm25 = signal(&[("a", 3.0), ("b", 2.0), ("c", 0.0)]);
        let semantic = signal(&[("a", 0.0), ("b", 2.0), ("c", 3.0)]);
        let selection = variant_selection("weighted-union", 0.5, 60.0, "mean", 48);
        let fused = select_coarse_ordering(&bm25, &semantic, &selection).expect("dispatch");
        let names: Vec<&str> = fused.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a", "c"]);
        assert!((fused[0].score - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn lexical_signal_matches_bm25_ordering_on_dev_fixtures() {
        let cases = load_cases(None).expect("builtin corpus");
        let partition = partition_cases(&cases);
        let dev: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .take(4)
            .map(|index| cases[*index].clone())
            .collect();
        assert_eq!(dev.len(), 4);
        for case in &dev {
            assert_eq!(
                advisor_lexical_ordering(case),
                bm25_ordering(case),
                "advisor lexical signal must reuse catalog BM25 ordering"
            );
        }
    }

    #[test]
    fn coarse_ordering_is_deterministic_under_permutation() {
        let bm25 = signal(&[("b", 1.0), ("a", 1.0), ("c", 0.5)]);
        let semantic = signal(&[("c", 0.5), ("a", 0.5), ("b", 0.5)]);
        let selections = [
            variant_selection("weighted-union", 0.25, 60.0, "mean", 48),
            variant_selection("rrf", 0.5, 120.0, "cls", 64),
            variant_selection("max", 0.5, 60.0, "mean", 48),
        ];
        for selection in &selections {
            let first = select_coarse_ordering(&bm25, &semantic, selection).expect("dispatch");
            let permuted_bm25 = signal(&[("c", 0.5), ("b", 1.0), ("a", 1.0)]);
            let permuted_semantic = signal(&[("a", 0.5), ("b", 0.5), ("c", 0.5)]);
            assert_eq!(
                select_coarse_ordering(&permuted_bm25, &permuted_semantic, selection)
                    .expect("dispatch"),
                first,
                "fusion output must not depend on input order"
            );
            // Name tiebreaks stay stable: the tied a/b pair keeps a first.
            assert_eq!(first[0].name, "a");
        }
    }

    /// Stub semantic scorer with exact 2-D embeddings, so cosine scores
    /// are hand-computable: query [1,0] gives a=1.0, b=0.0,
    /// c=1/sqrt(2).
    struct StubSemanticScorer {
        descriptors: BTreeMap<String, Vec<f32>>,
        fail_query: bool,
        fail_descriptors: bool,
        seen_pooling: RefCell<Vec<PoolingStrategy>>,
    }

    impl StubSemanticScorer {
        fn working() -> Self {
            Self {
                descriptors: BTreeMap::from([
                    ("tool-a".to_string(), vec![1.0, 0.0]),
                    ("tool-b".to_string(), vec![0.0, 1.0]),
                    ("tool-c".to_string(), vec![1.0, 1.0]),
                ]),
                fail_query: false,
                fail_descriptors: false,
                seen_pooling: RefCell::new(Vec::new()),
            }
        }
    }

    impl SemanticScorer for StubSemanticScorer {
        fn tokenizer_version(&self) -> String {
            "stub-encoder-v1".into()
        }

        fn encode_query(&self, _context: &str, pooling: PoolingStrategy) -> Result<Vec<f32>> {
            self.seen_pooling.borrow_mut().push(pooling);
            if self.fail_query {
                return Err(anyhow!("stub query failure"));
            }
            Ok(vec![1.0, 0.0])
        }

        fn encode_descriptor(
            &self,
            descriptor: &str,
            pooling: PoolingStrategy,
        ) -> Result<Vec<f32>> {
            self.seen_pooling.borrow_mut().push(pooling);
            if self.fail_descriptors {
                return Err(anyhow!("stub descriptor failure"));
            }
            let name = descriptor
                .strip_prefix("canonical name: ")
                .and_then(|rest| rest.split(';').next())
                .ok_or_else(|| anyhow!("stub cannot parse descriptor"))?;
            self.descriptors
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("stub has no vector for {name}"))
        }
    }

    fn stub_case(id: &str) -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: id.into(),
            context: format!("stub context {id}"),
            candidates: vec![
                synthetic_candidate("tool-a", "deferred"),
                synthetic_candidate("tool-b", "deferred"),
                synthetic_candidate("tool-c", "deferred"),
                synthetic_candidate(&format!("{id}-immediate"), "immediate"),
                synthetic_candidate(&format!("{id}-denied"), "denied"),
            ],
            relevance: BTreeMap::from([("tool-a".to_string(), 3), (format!("{id}-denied"), 3)]),
            preferred_order: vec!["tool-a".into()],
            none: false,
            tags: vec![],
            group_id: id.into(),
            provenance: "r001-test".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn variant_retrieval_matches_reference_fusion_through_stub() {
        let case = stub_case("stub");
        let selection = variant_selection("weighted-union", 0.5, 60.0, "mean", 48);
        let mut retriever = VariantRetriever::new(StubSemanticScorer::working());
        let result = retriever
            .retrieve(&case, "surface-a", &selection, 16)
            .expect("retrieve");
        assert!(!result.fallback_to_bm25);
        assert_eq!(result.eligible_count, 3);
        assert_eq!(result.cache_entries, 3);
        assert_eq!(result.mode, selection.point_mode());
        let lexical: Vec<(String, f64)> = advisor_lexical_ordering(&case)
            .into_iter()
            .map(|entry| (entry.name, entry.score))
            .collect();
        let semantic = vec![
            (
                "tool-a".to_string(),
                variant_cosine(&[1.0, 0.0], &[1.0, 0.0]),
            ),
            (
                "tool-b".to_string(),
                variant_cosine(&[1.0, 0.0], &[0.0, 1.0]),
            ),
            (
                "tool-c".to_string(),
                variant_cosine(&[1.0, 0.0], &[1.0, 1.0]),
            ),
        ];
        let mut expected: Vec<(String, f64)> =
            select_coarse_ordering(&lexical, &semantic, &selection)
                .expect("reference fusion")
                .into_iter()
                .map(|entry| (entry.name, entry.score))
                .collect();
        sort_scored(&mut expected);
        assert_eq!(result.names.len(), 3);
        for (index, (name, score)) in expected.iter().enumerate() {
            assert_eq!(&result.names[index], name);
            assert!((result.scores[index] - score).abs() < 1e-12);
        }
        // Non-deferred candidates never enter, even when labeled relevant.
        assert!(
            !result.names.iter().any(|name| name.ends_with("denied")),
            "denied tools must never enter variant retrieval"
        );
    }

    #[test]
    fn encoder_failure_falls_back_to_bm25_without_availability_gate() {
        let case = stub_case("fallback");
        let selection = variant_selection("rrf", 0.5, 60.0, "mean", 48);
        for (fail_query, fail_descriptors) in [(true, false), (false, true)] {
            let mut retriever = VariantRetriever::new(StubSemanticScorer {
                descriptors: StubSemanticScorer::working().descriptors,
                fail_query,
                fail_descriptors,
                seen_pooling: RefCell::new(Vec::new()),
            });
            let result = retriever
                .retrieve(&case, "surface-a", &selection, 2)
                .expect("fallback retrieve");
            assert!(result.fallback_to_bm25);
            let expected: Vec<String> = advisor_lexical_ordering(&case)
                .into_iter()
                .take(2)
                .map(|entry| entry.name)
                .collect();
            assert_eq!(result.names, expected);
        }
        VariantRetriever::new(StubSemanticScorer::working())
            .retrieve(&case, "surface-a", &selection, 0)
            .expect_err("zero K fails closed");
    }

    #[test]
    fn pooling_variants_are_stable_and_never_collide_in_cache() {
        let case = stub_case("pooling");
        let mut retriever = VariantRetriever::new(StubSemanticScorer::working());
        let mean_selection = variant_selection("max", 0.5, 60.0, "mean", 48);
        let cls_selection = variant_selection("max", 0.5, 60.0, "cls", 48);
        let mean = retriever
            .retrieve(&case, "surface-a", &mean_selection, 16)
            .expect("mean retrieve");
        let cls = retriever
            .retrieve(&case, "surface-a", &cls_selection, 16)
            .expect("cls retrieve");
        // The stub returns identical vectors per pooling, so orderings
        // match; the assertion that matters is cache separation below.
        assert_eq!(mean.names, cls.names);
        assert_eq!(retriever.cache.len(), 6);
        {
            let poolings = retriever.scorer.seen_pooling.borrow();
            let poolings = poolings.iter().collect::<Vec<_>>();
            assert!(poolings.contains(&&PoolingStrategy::Mean));
            assert!(poolings.contains(&&PoolingStrategy::Cls));
        }
        // Surface change invalidates, same surface reuses.
        let before = retriever.cache.len();
        retriever
            .retrieve(&case, "surface-a", &mean_selection, 16)
            .expect("cached retrieve");
        assert_eq!(retriever.cache.len(), before);
        retriever
            .retrieve(&case, "surface-b", &mean_selection, 16)
            .expect("new surface retrieve");
        assert_eq!(retriever.cache.len(), 3);
    }

    #[test]
    fn variant_cache_keys_never_contain_user_context() {
        let marker = "user-context-marker-7f3a9c";
        let mut case = stub_case("hygiene");
        case.context = format!("{marker} inspect secret project files");
        let selection = variant_selection("weighted-union", 0.25, 30.0, "mean", 64);
        let mut retriever = VariantRetriever::new(StubSemanticScorer::working());
        retriever
            .retrieve(&case, "surface-a", &selection, 16)
            .expect("retrieve");
        assert!(!retriever.cache.is_empty());
        for key in retriever.cache.keys() {
            let rendered = format!("{key:?}");
            assert!(
                !rendered.contains(marker),
                "cache key must never contain user context: {rendered}"
            );
        }
    }

    #[test]
    fn variant_point_identity_uses_universe_not_shortlist_k() {
        use super::super::sequence_retrieval::{select_retrieval_point, RetrievalFrontierPoint};
        let selection = variant_selection("weighted-union", 0.25, 60.0, "cls", 48);
        let point = |universe: usize, k: usize| RetrievalFrontierPoint {
            mode: selection.point_mode(),
            k,
            cases: 1,
            relevant_tools: 1,
            recovered_tools: 1,
            recall: 1.0,
            mean_latency_ms: 0.0,
            max_latency_ms: 0,
            candidate_universe_size: universe,
            shortlist_k: k,
            eligible_relevant_tools: 1,
            recovered_relevant_tools: 1,
        };
        let points = vec![point(64, 16), point(64, 24)];
        assert!(select_retrieval_point(&points, 64, 16, &selection.point_mode()).is_ok());
        select_retrieval_point(&points, 128, 16, &selection.point_mode())
            .expect_err("missing universe fails closed");
        select_retrieval_point(&points, 64, 16, "rrf").expect_err("mode mismatch fails closed");
        let duplicated = vec![point(64, 16), point(64, 16)];
        select_retrieval_point(&duplicated, 64, 16, &selection.point_mode())
            .expect_err("duplicate fails closed");
    }

    /// Deterministic stub chunk scorer: scores derive from the name only,
    /// so permuted pools must promote identical sets.
    struct StubChunkScorer {
        max_chunk: usize,
    }

    impl ChunkRankScorer for StubChunkScorer {
        fn scorer_name(&self) -> &'static str {
            "stub-chunk"
        }

        fn max_chunk_candidates(&self) -> usize {
            self.max_chunk
        }

        fn score_chunk(
            &self,
            _case: &ToolAdvisorCase,
            candidates: &[ToolAdvisorCandidate],
        ) -> Result<(Vec<RankedCandidate>, usize, usize)> {
            let ranked = candidates
                .iter()
                .map(|candidate| {
                    let first = candidate.name.as_bytes().first().copied().unwrap_or(0);
                    RankedCandidate {
                        name: candidate.name.clone(),
                        score: candidate.name.len() as f64 + f64::from(first) / 256.0,
                    }
                })
                .collect();
            Ok((ranked, candidates.len(), 0))
        }
    }

    fn rerank_case() -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: "rerank".into(),
            context: "rerank context".into(),
            candidates: vec![
                synthetic_candidate("a", "deferred"),
                synthetic_candidate("bb", "deferred"),
                synthetic_candidate("ccc", "deferred"),
                synthetic_candidate("dddd", "deferred"),
                synthetic_candidate("ee", "deferred"),
                synthetic_candidate("rerank-immediate", "immediate"),
            ],
            relevance: BTreeMap::from([("dddd".to_string(), 3)]),
            preferred_order: vec!["dddd".into()],
            none: false,
            tags: vec![],
            group_id: "rerank".into(),
            provenance: "r001-test".into(),
            semantic_group: String::new(),
            leakage_group: String::new(),
            task_family: String::new(),
            tool_family: String::new(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn rerank_pool_chunks_counts_cost_and_promotes_by_score() {
        // Hand scores: dddd=4+100/256, ccc=3+99/256, ee=2+101/256,
        // bb=2+98/256, a=1+97/256. Top-3 is dddd, ccc, ee.
        let case = rerank_case();
        let scorer = StubChunkScorer { max_chunk: 2 };
        let pool: Vec<String> = ["a", "bb", "ccc", "dddd", "ee"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let report = rerank_pool(&scorer, &case, &pool, 3).expect("rerank");
        assert_eq!(report.chunks, 3);
        assert_eq!(report.forwards, 5);
        assert_eq!(report.dropped, 0);
        assert_eq!(report.pool_size, 5);
        let promoted: Vec<&str> = report
            .promoted
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(promoted, vec!["dddd", "ccc", "ee"]);
    }

    #[test]
    fn rerank_pool_is_permutation_invariant() {
        let case = rerank_case();
        let scorer = StubChunkScorer { max_chunk: 2 };
        let forward: Vec<String> = ["a", "bb", "ccc", "dddd", "ee"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut reversed = forward.clone();
        reversed.reverse();
        let first = rerank_pool(&scorer, &case, &forward, 3).expect("rerank");
        let second = rerank_pool(&scorer, &case, &reversed, 5).expect("rerank");
        // Same promoted set regardless of input order (name-sorted
        // chunking); the K=5 report holds the full ordered pool.
        let full: Vec<&str> = second
            .promoted
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(full, vec!["dddd", "ccc", "ee", "bb", "a"]);
        let promoted: Vec<&str> = first
            .promoted
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(promoted, vec!["dddd", "ccc", "ee"]);
        for (left, right) in first.promoted.iter().zip(second.promoted.iter().take(3)) {
            assert_eq!(left.name, right.name);
            assert!((left.score - right.score).abs() < 1e-12);
        }
    }

    #[test]
    fn rerank_pool_revalidates_the_deferred_universe() {
        let case = rerank_case();
        let scorer = StubChunkScorer { max_chunk: 16 };
        rerank_pool(&scorer, &case, &["a".to_string()], 0).expect_err("zero K fails closed");
        rerank_pool(&scorer, &case, &[], 2).expect_err("empty pool fails closed");
        rerank_pool(&scorer, &case, &["rerank-immediate".to_string()], 1)
            .expect_err("non-deferred pool member fails closed");
        rerank_pool(&scorer, &case, &["ghost-tool".to_string()], 1)
            .expect_err("unknown pool member fails closed");
        rerank_pool(
            &scorer,
            &case,
            &["a".to_string(), "ghost-tool".to_string()],
            2,
        )
        .expect_err("partially unknown pool fails closed");
    }

    #[test]
    fn variant_grid_partitions_into_nine_fusion_settings() {
        assert_eq!(all_variant_selections().len(), 180);
        let settings = fusion_settings();
        assert_eq!(settings.len(), 9);
        // Every grid mode belongs to exactly one setting's member set.
        let mut covered = BTreeSet::new();
        for (fusion, alpha, rrf_k) in &settings {
            for (member_alpha, member_rrf) in setting_members(fusion, *alpha, *rrf_k) {
                for pooling in POOLING_VARIANTS {
                    for rerank_n in RERANK_CANDIDATE_NS {
                        let member = VariantSelection::new(
                            fusion,
                            member_alpha,
                            member_rrf,
                            pooling,
                            rerank_n,
                        )
                        .expect("member");
                        assert!(
                            covered.insert(member.point_mode()),
                            "duplicate grid mode {}",
                            member.point_mode()
                        );
                    }
                }
            }
            // The canonical member is itself a grid point.
            for pooling in POOLING_VARIANTS {
                for rerank_n in RERANK_CANDIDATE_NS {
                    let canonical = setting_canonical(fusion, *alpha, *rrf_k, pooling, rerank_n)
                        .expect("canonical");
                    assert!(is_setting_canonical(&canonical));
                    assert!(covered.contains(&canonical.point_mode()));
                }
            }
        }
        assert_eq!(covered.len(), 180);
        assert!(setting_members("ghost", 0.5, 60.0).is_empty());
        assert!(setting_canonical("ghost", 0.5, 60.0, "mean", 48).is_err());
    }

    #[test]
    fn sweep_checkpoint_resumes_only_on_matching_fingerprint_and_universe() {
        let dir = std::env::temp_dir().join("r001-checkpoint-unit");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = checkpoint_path(&dir.display().to_string(), 64);
        let checkpoint = R001UniverseCheckpoint {
            sweep_fingerprint: "fp".into(),
            universe_size: 64,
            fixture_fingerprint: "fix".into(),
            points: Vec::new(),
            arms: Vec::new(),
            authority_violations: 0,
        };
        std::fs::write(
            &file,
            serde_json::to_vec_pretty(&checkpoint).expect("serialize"),
        )
        .expect("write");
        let bytes = std::fs::read(&file).expect("read");
        let loaded: R001UniverseCheckpoint = serde_json::from_slice(&bytes).expect("round-trip");
        assert_eq!(loaded.fixture_fingerprint, "fix");
        assert!(checkpoint_accepts(
            &loaded.sweep_fingerprint,
            loaded.universe_size,
            "fp",
            64
        ));
        assert!(!checkpoint_accepts(
            &loaded.sweep_fingerprint,
            loaded.universe_size,
            "other",
            64
        ));
        assert!(!checkpoint_accepts(
            &loaded.sweep_fingerprint,
            loaded.universe_size,
            "fp",
            128
        ));
        std::fs::remove_file(&file).ok();
    }

    fn r001_point(
        mode: &str,
        universe: usize,
        k: usize,
        recall: f64,
        mean_ms: f64,
    ) -> R001FrontierPoint {
        let total = 1000;
        let recovered = (recall * total as f64) as usize;
        R001FrontierPoint {
            schema_version: RETRIEVAL_SCHEMA_VERSION,
            mode: mode.into(),
            fusion: "weighted-union".into(),
            alpha: 0.5,
            rrf_k: 60.0,
            pooling: "mean".into(),
            rerank_n: 48,
            reranked: false,
            rerank_arm: None,
            universe_size: universe,
            k,
            cases: 62,
            eligible_relevant: total,
            recovered_relevant: recovered,
            recall: recovered as f64 / total as f64,
            mean_latency_ms: mean_ms,
            p95_latency_ms: mean_ms,
            max_latency_ms: mean_ms as u128,
            mean_rerank_ms_per_case: 0.0,
            mean_forwards_per_case: 0.0,
            fallback_cases: 0,
            cache_entries: 0,
        }
    }

    fn r001_universe_points(
        mode: &str,
        universe: usize,
        k: usize,
        recall: f64,
        mean_ms: f64,
    ) -> Vec<R001FrontierPoint> {
        vec![r001_point(mode, universe, k, recall, mean_ms)]
    }

    #[test]
    fn r001_primary_selection_picks_smallest_cheapest_clearing_point() {
        // One mode clearing at K=24 across universes.
        let points_64 = r001_universe_points("m", 64, 24, 1.0, 5.0);
        let points_128 = r001_universe_points("m", 128, 24, 0.99, 5.0);
        let points_256 = r001_universe_points("m", 256, 24, 0.96, 5.0);
        let selected =
            select_r001_primary(&points_64, &points_128, &points_256, 0).expect("selection");
        assert_eq!((selected.mode.as_str(), selected.k), ("m", 24));
        assert!((selected.recall_256 - 0.96).abs() < 1e-9);
        // Missing 256 gate fails closed.
        let weak_256 = r001_universe_points("m", 256, 24, 0.90, 5.0);
        assert!(select_r001_primary(&points_64, &points_128, &weak_256, 0).is_err());
        // Authority violations fail closed.
        assert!(select_r001_primary(&points_64, &points_128, &points_256, 1).is_err());
        // Smaller K wins over lower latency at larger K.
        let mut all_64 = points_64.clone();
        all_64.extend(r001_universe_points("fast", 64, 16, 1.0, 50.0));
        let mut all_128 = points_128.clone();
        all_128.extend(r001_universe_points("fast", 128, 16, 0.99, 50.0));
        let mut all_256 = points_256.clone();
        all_256.extend(r001_universe_points("fast", 256, 16, 0.96, 50.0));
        let selected = select_r001_primary(&all_64, &all_128, &all_256, 0).expect("selection");
        assert_eq!((selected.mode.as_str(), selected.k), ("fast", 16));
        // At equal K the cheaper mode wins; exact ties break by name.
        let mut cheap_64 = r001_universe_points("aaa", 64, 16, 1.0, 5.0);
        cheap_64.extend(r001_universe_points("zzz", 64, 16, 1.0, 5.0));
        let mut cheap_128 = r001_universe_points("aaa", 128, 16, 0.99, 6.0);
        cheap_128.extend(r001_universe_points("zzz", 128, 16, 0.99, 1.0));
        let mut cheap_256 = r001_universe_points("aaa", 256, 16, 0.96, 5.0);
        cheap_256.extend(r001_universe_points("zzz", 256, 16, 0.96, 5.0));
        let selected =
            select_r001_primary(&cheap_64, &cheap_128, &cheap_256, 0).expect("selection");
        assert_eq!(selected.mode.as_str(), "aaa");
        // Modes missing any universe are skipped, never defaulted.
        let partial_64 = r001_universe_points("partial", 64, 16, 1.0, 1.0);
        let mut mixed_64 = cheap_64.clone();
        mixed_64.extend(partial_64);
        let selected =
            select_r001_primary(&mixed_64, &cheap_128, &cheap_256, 0).expect("selection");
        assert_eq!(selected.mode.as_str(), "aaa");
    }

    #[test]
    fn r001_extended_selection_requires_gates_and_every_budget() {
        let limits = preregistration("unit").budgets;
        let points_64 = r001_universe_points("m", 64, 48, 1.0, 5.0);
        let points_128 = r001_universe_points("m", 128, 48, 0.99, 5.0);
        let points_256 = r001_universe_points("m", 256, 48, 0.96, 5.0);
        let good_latency = R001CandidateLatency {
            p95_ms: 100.0,
            max_ms: 500,
            mean_rerank_ms: 200.0,
        };
        let mut latencies = BTreeMap::new();
        latencies.insert(("m".to_string(), 48), good_latency);
        // Primary branch sees nothing at K<=32, so only extended can win.
        assert!(select_r001_primary(&points_64, &points_128, &points_256, 0).is_err());
        let (candidate, evidence) = select_r001_extended(
            &points_64,
            &points_128,
            &points_256,
            0,
            &latencies,
            2.0,
            100,
            3600,
            1000,
            &limits,
        )
        .expect("extended selection");
        assert_eq!((candidate.mode.as_str(), candidate.k), ("m", 48));
        assert!((evidence.retrieval_p95_ms - 100.0).abs() < 1e-9);
        assert_eq!(evidence.retrieval_max_ms, 500);
        assert!((evidence.rerank_per_case_ms - 200.0).abs() < 1e-9);
        // Each budget breach fails closed with the gates still clearing.
        for breach in [
            R001CandidateLatency {
                p95_ms: 6000.0,
                max_ms: 500,
                mean_rerank_ms: 200.0,
            },
            R001CandidateLatency {
                p95_ms: 100.0,
                max_ms: 40_000,
                mean_rerank_ms: 200.0,
            },
            R001CandidateLatency {
                p95_ms: 100.0,
                max_ms: 500,
                mean_rerank_ms: 6000.0,
            },
        ] {
            let mut breached = BTreeMap::new();
            breached.insert(("m".to_string(), 48), breach);
            assert!(
                select_r001_extended(
                    &points_64,
                    &points_128,
                    &points_256,
                    0,
                    &breached,
                    2.0,
                    100,
                    3600,
                    1000,
                    &limits,
                )
                .is_err(),
                "budget breach must fail closed"
            );
        }
        assert!(
            select_r001_extended(
                &points_64,
                &points_128,
                &points_256,
                0,
                &latencies,
                20.0,
                100,
                3600,
                1000,
                &limits,
            )
            .is_err(),
            "cold-load breach must fail closed"
        );
        assert!(
            select_r001_extended(
                &points_64,
                &points_128,
                &points_256,
                1,
                &latencies,
                2.0,
                100,
                3600,
                1000,
                &limits,
            )
            .is_err(),
            "violations must fail closed"
        );
    }

    #[test]
    fn candidate_latencies_aggregate_worst_case_across_universes() {
        let points = vec![
            r001_point("m", 64, 48, 1.0, 10.0),
            r001_point("m", 128, 48, 1.0, 30.0),
            r001_point("m", 256, 48, 1.0, 20.0),
        ];
        let aggregated = aggregate_candidate_latencies(&points);
        let latency = aggregated
            .get(&("m".to_string(), 48))
            .expect("aggregated latency");
        assert!((latency.p95_ms - 30.0).abs() < 1e-9);
        assert_eq!(latency.max_ms, 30);
        let evidence = evidence_for_candidate(latency, 2.0, 100, 3600, 1000);
        assert!((evidence.retrieval_p95_ms - 30.0).abs() < 1e-9);
        assert_eq!(evidence.sweep_wallclock_s, 3600);
    }

    /// Predeclared R001 sweep (protocol
    /// `r001-preregistered-retrieval-architecture-v1`).
    ///
    /// Measures the M002 variant grid on dev universes, selects at most
    /// one operating point (primary K<=32 preferred, bounded extended
    /// K only with all budgets met), attributes residual misses at the
    /// frozen point, and re-attaches the unchanged M004 promotion
    /// separator there. Run explicitly for closure evidence:
    /// `cargo test --locked --features tool-advisor-encoder-training
    /// -p codegg --lib --
    /// tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep
    /// --ignored --nocapture`
    #[test]
    #[ignore]
    fn r001_extended_frontier_sweep() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let encoder_manifest = root.join(PREREG_ENCODER_MANIFEST);
        let ranker_artifact = root.join(PREREG_RANKER_ARTIFACT);
        if !encoder_manifest.exists() || !ranker_artifact.exists() {
            eprintln!("SKIP: reference encoder assets or frozen ranker absent in this environment");
            return;
        }
        let output_dir = root.join(PREREG_OUTPUT_DIR);
        let report = run_r001_sweep(&output_dir).expect("r001 sweep runs to verdict");
        for universe_size in PREREG_UNIVERSES {
            eprintln!(
                "r001 frontier universe {universe_size} (mode recall@16/24/32/48/64 mean-ms):"
            );
            let mut rows: BTreeMap<String, Vec<&R001FrontierPoint>> = BTreeMap::new();
            for point in report
                .points
                .iter()
                .filter(|point| point.universe_size == universe_size)
            {
                rows.entry(point.mode.clone()).or_default().push(point);
            }
            for (mode, mut row) in rows {
                row.sort_by_key(|point| point.k);
                let recalls: Vec<String> = row
                    .iter()
                    .map(|point| format!("{:.4}", point.recall))
                    .collect();
                let mean_ms = row
                    .first()
                    .map(|point| point.mean_latency_ms)
                    .unwrap_or(0.0);
                let rerank_ms = row
                    .first()
                    .map(|point| point.mean_rerank_ms_per_case)
                    .unwrap_or(0.0);
                let marker = if row.first().map(|point| point.reranked).unwrap_or(false) {
                    "reranked"
                } else {
                    "coarse   "
                };
                eprintln!(
                    "  {marker} {mode} {} mean={mean_ms:.1}ms rerank={rerank_ms:.1}ms",
                    recalls.join(" "),
                );
            }
        }
        match &report.selection {
            Some(selection) => {
                eprintln!(
                    "r001 frozen: {} K={} extended={} reranked={} \
                     (64:{:.4} 128:{:.4} 256:{:.4}) violations={} wallclock={}s",
                    selection.mode,
                    selection.k,
                    selection.extended,
                    selection.reranked,
                    selection.recall_64,
                    selection.recall_128,
                    selection.recall_256,
                    selection.authority_violations,
                    report.resources.sweep_wallclock_s,
                );
                if let Some(promotion) = &report.promotion {
                    eprintln!(
                        "r001 promotion: threshold={:.2} recall={:.4} no-tool={:.4} \
                         irrelevant={:.4} max_promos={} consistency={:.3} crossings={}",
                        promotion.outcome.threshold,
                        promotion.outcome.relevant_recall,
                        promotion.outcome.no_tool_promotion_rate,
                        promotion.outcome.irrelevant_promotion_rate,
                        promotion.outcome.max_promotions_observed,
                        promotion.order.identity_consistency,
                        promotion.order.threshold_crossing_cases,
                    );
                }
                eprintln!(
                    "r001 residual misses at frozen point: {}",
                    report.attribution.len()
                );
            }
            None => panic!(
                "r001 negative: {}",
                report.negative_summary.as_deref().unwrap_or("unknown")
            ),
        }
    }

    #[test]
    fn live_variant_paths_reproduce_m004_endpoints() {
        use super::super::sequence_encoder::CandleBertSequenceEncoder;
        use super::super::sequence_retrieval::{HybridRetriever, RetrievalMode};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifest_path = root.join(PREREG_ENCODER_MANIFEST);
        if !manifest_path.exists() {
            eprintln!("SKIP: reference encoder assets absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let encoder =
            CandleBertSequenceEncoder::load(&manifest_path, &device).expect("encoder load");
        let case = stub_case("live");
        let surface = "live-surface";
        let mut hybrid = HybridRetriever::new(&encoder);
        let mut variant = VariantRetriever::new(EncoderSemanticScorer::new(&encoder));
        // Alpha 1.0 is semantic-only: ordering must equal M004 semantic.
        let semantic_only = variant_selection("weighted-union", 1.0, 60.0, "mean", 48);
        let variant_semantic = variant
            .retrieve(&case, surface, &semantic_only, 3)
            .expect("semantic-only retrieve");
        let m004_semantic = hybrid
            .retrieve(&case, surface, RetrievalMode::Semantic, 3)
            .expect("M004 semantic retrieve");
        assert!(!variant_semantic.fallback_to_bm25);
        assert_eq!(variant_semantic.names, m004_semantic.names);
        // Alpha 0.0 is BM25-only: ordering must equal M004 BM25.
        let lexical_only = variant_selection("weighted-union", 0.0, 60.0, "mean", 48);
        let variant_lexical = variant
            .retrieve(&case, surface, &lexical_only, 3)
            .expect("lexical-only retrieve");
        let m004_bm25 = hybrid
            .retrieve(&case, surface, RetrievalMode::Bm25, 3)
            .expect("M004 BM25 retrieve");
        assert_eq!(variant_lexical.names, m004_bm25.names);
        // Cls pooling runs cleanly on the same path.
        let cls = variant_selection("max", 0.5, 60.0, "cls", 48);
        let cls_result = variant
            .retrieve(&case, surface, &cls, 3)
            .expect("cls retrieve");
        assert!(!cls_result.fallback_to_bm25);
        assert_eq!(cls_result.names.len(), 3);
        eprintln!(
            "live variant endpoints match M004: semantic={:?} bm25={:?}",
            variant_semantic.names, variant_lexical.names
        );
    }

    #[test]
    fn live_frozen_ranker_reranks_and_rejects_wrong_architecture() {
        use super::super::sequence_ranking::load_artifact;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let artifact_path = root.join(PREREG_RANKER_ARTIFACT);
        if !artifact_path.exists() {
            eprintln!("SKIP: frozen span-packed ranker absent in this environment");
            return;
        }
        let device = candle_core::Device::Cpu;
        let mut ranker =
            load_artifact(&artifact_path, &device).expect("load frozen span-packed ranker");
        // Frozen binding: the loader itself verified the head hash; the
        // architecture and dev partition pin the selected artifact.
        assert_eq!(
            ranker.manifest.architecture, RANKING_ARCHITECTURE_SPAN_PACKED,
            "re-rank binds to the frozen span-packed ranker"
        );
        assert_eq!(
            ranker.manifest.dev_partition_fingerprint, EXPECTED_DEV_PARTITION_FINGERPRINT,
            "ranker dev partition must match the frozen corpus"
        );
        let scorer = FrozenRankerScorer::new(&ranker).expect("frozen scorer binds");
        let case = rerank_case();
        let pool: Vec<String> = ["a", "bb", "ccc"].iter().map(ToString::to_string).collect();
        let report = rerank_pool(&scorer, &case, &pool, 2).expect("frozen rerank");
        assert_eq!(report.promoted.len(), 2);
        assert_eq!(report.dropped, 0);
        assert!(report.forwards >= 1, "ranker cost must be counted");
        eprintln!(
            "live frozen rerank promotes {:?} in {} forwards",
            report
                .promoted
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            report.forwards
        );
        ranker.manifest.architecture = "bogus-architecture".into();
        assert!(
            FrozenRankerScorer::new(&ranker).is_err(),
            "wrong architecture fails closed"
        );
    }
}
