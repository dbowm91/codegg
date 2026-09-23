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

use super::{
    baseline_prediction, dataset_fingerprint, load_cases, partition_cases, RankedCandidate,
    ToolAdvisorCase,
};
use crate::tool::catalog::SearchMode;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
}
