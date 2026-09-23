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
use super::sequence_encoder::{CandleBertSequenceEncoder, PoolingStrategy};
use super::sequence_ranking::{SequenceRanker, RANKING_ARCHITECTURE_SPAN_PACKED};
use super::sequence_retrieval::RETRIEVAL_SCHEMA_VERSION;
use super::{
    baseline_prediction, dataset_fingerprint, load_cases, partition_cases, RankedCandidate,
    ToolAdvisorCandidate, ToolAdvisorCase,
};
use crate::tool::catalog::SearchMode;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
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
