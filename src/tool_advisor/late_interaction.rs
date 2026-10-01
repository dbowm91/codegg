//! Tool-selection advisor late-interaction retriever M001.
//!
//! Frozen exact token-level MaxSim over the pinned MiniLM encoder with zero
//! learned retrieval parameters. M001 answers one question: does preserving
//! token-level representations recover query/tool signal lost by mean
//! pooling?
//!
//! Frozen contract (`late-interaction-m001-v1`, preregistered before any dev
//! measurement in `assets/tool-advisor/late-interaction-m001-preregistration.json`):
//!
//! - query/descriptor texts: Signal V2 `flat_text` (no aliases, fields, or
//!   expansions);
//! - token caps: [`QUERY_TOKEN_CAP`] / [`DESCRIPTOR_TOKEN_CAP`] content
//!   tokens, deterministic tail truncation;
//! - special tokens (`[CLS]`, `[SEP]`) and padding excluded from scoring;
//! - every token vector L2-normalized independently (`NORM_EPSILON` guard);
//! - `score(Q,D) = mean_i max_j dot(norm(q_i), norm(d_j))`;
//! - candidate tie-break: score descending, name ascending;
//! - modes: `exact-maxsim-v1`, plus one fixed `lexical-maxsim-union-v1`
//!   (min-max normalized field-weighted Signal V2 lexical + MaxSim,
//!   alpha 0.5, mirroring the frozen M002 fusion arithmetic);
//! - descriptor cache stores token vectors only, keyed by contract version
//!   + tokenizer/weights hashes + descriptor text hash; never query/context.
//!
//! The M002 best-mode (`normalized-union-v2`) ordering is reproduced with
//! frozen M002 code-path arithmetic for residual-miss attribution ONLY. It
//! is not a preregistered mode and selects nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::retrieval_relevance::{
    build_derived_view, eligible_deferred_names_local, expand_universe_local,
};
use super::retrieval_signal::{
    EXPECTED_DERIVED_VIEW_FINGERPRINT, MINILM_DIM, PREREG_ENCODER_MANIFEST,
};
use super::retrieval_signal_v2::{
    lexical_ordering, RetrievalDescriptorV2, RetrievalQueryV2, FRONTIER_KS, FRONTIER_UNIVERSES,
};
use super::sequence_encoder::CandleBertSequenceEncoder;
use super::{builtin_cases, partition_cases, unknown_tool_holdout, ToolAdvisorCase};

/// Frozen M001 contract version, bound into prereg, cache keys, and receipt.
pub const LATE_INTERACTION_CONTRACT_VERSION: &str = "late-interaction-m001-v1";
/// Preregistration receipt path (Commit A, before dev measurement).
pub const M001_PREREG_ASSET: &str =
    "assets/tool-advisor/late-interaction-m001-preregistration.json";
/// Dev frontier result path (Commit B, one frozen run).
pub const M001_RECEIPT_ASSET: &str = "assets/tool-advisor/late-interaction-m001-frontier.json";
/// Descriptor token-vector cache key version.
pub const M001_CACHE_VERSION: &str = "late-interaction-cache-v1";
/// Preregistered token caps (content tokens, special tokens excluded).
/// Selected from train-only query evidence and static descriptor-length
/// evidence (see prereg receipt); each cap is <= 128.
pub const QUERY_TOKEN_CAP: usize = 96;
pub const DESCRIPTOR_TOKEN_CAP: usize = 96;
/// L2 normalization epsilon for token vectors before MaxSim.
pub const NORM_EPSILON: f32 = 1e-12;
/// Fixed fusion weight for `lexical-maxsim-union-v1` (mirrors M002).
pub const UNION_ALPHA: f64 = 0.5;
/// Lexical arm of the union mode (existing field-weighted Signal V2 score).
pub const UNION_LEXICAL_MODE: &str = "field-weighted-bm25-v2";
/// Exact preregistered modes. No alpha/RRF sweep.
pub const M001_MODES: [&str; 2] = ["exact-maxsim-v1", "lexical-maxsim-union-v1"];
/// Attribution-only reproduction of the frozen M002 best mode (u64 only).
pub const M002_ATTRIBUTION_MODE: &str = "normalized-union-v2";
/// Frozen retrieval gates (u64/u128/u256 at K<=32, zero violations).
pub const M001_GATES: [(usize, f64); 3] = [(64, 0.99), (128, 0.98), (256, 0.95)];
/// Hard resource guards for M001 architecture viability.
pub const MAX_CACHE_BYTES_256: usize = 64 * 1024 * 1024;
pub const MAX_WARM_RETRIEVAL_P95_MS: u128 = 1000;

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// L2-normalize one token vector (epsilon guards the zero vector).
pub fn normalize_token(vector: &[f32]) -> Vec<f32> {
    let norm = vector
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt()
        .max(f64::from(NORM_EPSILON)) as f32;
    vector.iter().map(|x| x / norm).collect()
}

/// Exact MaxSim: mean over query tokens of the max cosine against any
/// descriptor token. Inputs must already be L2-normalized. Returns `None`
/// when either side is empty so callers fail closed.
pub fn maxsim_score(query: &[Vec<f32>], descriptor: &[Vec<f32>]) -> Option<f32> {
    if query.is_empty() || descriptor.is_empty() {
        return None;
    }
    let mut sum = 0.0f32;
    for q in query {
        let mut best = f32::NEG_INFINITY;
        for d in descriptor {
            let dot = q.iter().zip(d.iter()).map(|(a, b)| a * b).sum::<f32>();
            if dot > best {
                best = dot;
            }
        }
        sum += best;
    }
    Some(sum / query.len() as f32)
}

/// Deterministic candidate ordering: score descending, name ascending.
/// Unscorable (`NEG_INFINITY`) candidates sink to the name-ordered tail.
pub fn order_scored(mut scored: Vec<(String, f32)>) -> Vec<(String, f64)> {
    scored.sort_by(|left, right| left.0.cmp(&right.0));
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    scored
        .into_iter()
        .map(|(name, score)| (name, f64::from(score)))
        .collect()
}

/// Min-max normalize per query and average with `UNION_ALPHA` (exact mirror
/// of the frozen M002 `fuse_normalized_union_v2` arithmetic).
pub fn fuse_minmax_union(
    lexical: &[(String, f64)],
    semantic: &[(String, f64)],
) -> Vec<(String, f64)> {
    let normalize = |scores: &[(String, f64)]| -> BTreeMap<String, f64> {
        let max = scores.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
        let min = scores.iter().map(|(_, s)| *s).fold(f64::INFINITY, f64::min);
        let range = max - min;
        scores
            .iter()
            .map(|(n, s)| {
                let norm = if range <= f64::EPSILON {
                    0.0
                } else {
                    (s - min) / range
                };
                (n.clone(), norm)
            })
            .collect()
    };
    let lex_norm = normalize(lexical);
    let sem_norm = normalize(semantic);
    let mut names = BTreeSet::new();
    for (n, _) in lexical {
        names.insert(n.clone());
    }
    for (n, _) in semantic {
        names.insert(n.clone());
    }
    let mut fused: Vec<(String, f64)> = names
        .into_iter()
        .map(|n| {
            let l = lex_norm.get(&n).copied().unwrap_or(0.0);
            let s = sem_norm.get(&n).copied().unwrap_or(0.0);
            (n, (1.0 - UNION_ALPHA) * l + UNION_ALPHA * s)
        })
        .collect();
    fused.sort_by(|l, r| l.0.cmp(&r.0));
    fused.sort_by(|l, r| {
        r.1.partial_cmp(&l.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| l.0.cmp(&r.0))
    });
    fused
}

/// f64 cosine (exact mirror of the frozen M002 helper, for attribution).
fn cosine_f64(left: &[f32], right: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (l, r) in left.iter().zip(right.iter()) {
        dot += f64::from(*l) * f64::from(*r);
        left_norm += f64::from(*l) * f64::from(*l);
        right_norm += f64::from(*r) * f64::from(*r);
    }
    let denom = left_norm.sqrt() * right_norm.sqrt();
    if denom <= f64::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

// ---------------------------------------------------------------------------
// Descriptor token cache (descriptor vectors only, never query/context)
// ---------------------------------------------------------------------------

/// Cache key: contract version + encoder/tokenizer hashes + text hash.
fn cache_key(flat_text: &str, tokenizer_version: &str, weights_hash: &str) -> String {
    let bytes = format!("{M001_CACHE_VERSION}|{tokenizer_version}|{weights_hash}|{flat_text}");
    hex::encode(Sha256::digest(bytes.as_bytes()))
}

/// In-process descriptor token-vector cache with deterministic accounting.
#[derive(Debug, Default)]
pub struct DescriptorTokenCache {
    entries: BTreeMap<String, (Vec<u32>, Vec<Vec<f32>>)>,
    tokenizer_version: String,
    weights_hash: String,
}

impl DescriptorTokenCache {
    pub fn new(tokenizer_version: String, weights_hash: String) -> Self {
        Self {
            entries: BTreeMap::new(),
            tokenizer_version,
            weights_hash,
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Deterministic byte accounting: 4 bytes per id + 4 per float.
    pub fn bytes(&self) -> usize {
        self.entries
            .values()
            .map(|(ids, vectors)| {
                ids.len() * 4 + vectors.iter().map(|v| v.len() * 4).sum::<usize>()
            })
            .sum()
    }

    /// Get-or-encode one descriptor's token vectors (normalized on read).
    /// Returns normalized vectors; encodes once via `encode` on miss.
    /// The cache never observes query or context text: callers pass only
    /// static descriptor flat text.
    pub fn get_or_encode(
        &mut self,
        flat_text: &str,
        encode: impl FnOnce() -> Result<(Vec<u32>, Vec<Vec<f32>>)>,
    ) -> Result<Vec<Vec<f32>>> {
        let key = cache_key(flat_text, &self.tokenizer_version, &self.weights_hash);
        if let Some((_, vectors)) = self.entries.get(&key) {
            return Ok(vectors.clone());
        }
        let (ids, raw) = encode()?;
        let normalized: Vec<Vec<f32>> = raw.iter().map(|v| normalize_token(v)).collect();
        self.entries.insert(key, (ids, normalized.clone()));
        Ok(normalized)
    }
}

// ---------------------------------------------------------------------------
// Preregistration receipt
// ---------------------------------------------------------------------------

/// Commit-A preregistration: frozen before any dev retrieval measurement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M001Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub contract_version: String,
    pub query_token_cap: usize,
    pub descriptor_token_cap: usize,
    pub truncation: String,
    pub special_token_filter: String,
    pub norm_epsilon: f32,
    pub aggregation: String,
    pub tiebreak: String,
    pub modes: Vec<String>,
    pub union_alpha: f64,
    pub union_lexical_mode: String,
    pub cache_key_version: String,
    pub resource_limits: BTreeMap<String, String>,
    pub gates: BTreeMap<String, f64>,
    pub corpus_fingerprint: String,
    pub derived_view_fingerprint: String,
    pub signal_v2_protocol: String,
    pub minilm_manifest_hashes: BTreeMap<String, String>,
    pub minilm_architecture: String,
    pub query_cap_evidence: BTreeMap<String, String>,
    pub descriptor_cap_evidence: BTreeMap<String, String>,
    pub fingerprint: String,
}

/// Deterministic fingerprint (excludes `fingerprint` itself).
pub fn prereg_fingerprint(prereg: &M001Preregistration) -> Result<String> {
    let mut canonical = serde_json::to_value(prereg).context("serialize prereg")?;
    if let Some(map) = canonical.as_object_mut() {
        map.remove("fingerprint");
    }
    let bytes = serde_json::to_vec(&canonical).context("canonical prereg bytes")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn load_prereg(root: &Path) -> Result<M001Preregistration> {
    let bytes = std::fs::read(root.join(M001_PREREG_ASSET)).context("read M001 prereg")?;
    let prereg: M001Preregistration =
        serde_json::from_slice(&bytes).context("parse M001 prereg")?;
    let expected = prereg_fingerprint(&prereg).context("prereg fingerprint")?;
    if prereg.fingerprint != expected {
        return Err(anyhow!("M001 preregistration fingerprint mismatch"));
    }
    if prereg.contract_version != LATE_INTERACTION_CONTRACT_VERSION
        || prereg.query_token_cap != QUERY_TOKEN_CAP
        || prereg.descriptor_token_cap != DESCRIPTOR_TOKEN_CAP
        || prereg.modes != M001_MODES.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    {
        return Err(anyhow!("M001 preregistration drifted from code contract"));
    }
    Ok(prereg)
}

// ---------------------------------------------------------------------------
// Frontier receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M001FrontierPoint {
    pub mode: String,
    pub universe: usize,
    pub k: usize,
    pub inferable_relevant: usize,
    pub inferable_recovered: usize,
    pub inferable_recall: f64,
    pub violations: usize,
    pub passes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M001LabelRank {
    pub case_id: String,
    pub tool: String,
    pub relevance_class: String,
    pub task_family: String,
    pub maxsim_rank_u64: Option<usize>,
    pub maxsim_rank_u128: Option<usize>,
    pub maxsim_rank_u256: Option<usize>,
    pub union_rank_u64: Option<usize>,
    pub lexical_rank_u64: Option<usize>,
    pub m002_union_rank_u64: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct M001ResourceEvidence {
    pub reference_host: String,
    pub toolchain: String,
    pub descriptor_precompute_ms: u128,
    pub descriptor_cache_entries: usize,
    pub descriptor_cache_bytes: usize,
    pub descriptor_cache_bytes_256: usize,
    pub query_encode_p50_ms: f64,
    pub query_encode_p95_ms: f64,
    pub maxsim_p50_ms: f64,
    pub maxsim_p95_ms: f64,
    pub total_retrieval_p50_ms: f64,
    pub total_retrieval_p95_ms: f64,
    pub total_retrieval_max_ms: u128,
    pub query_token_count_p50: usize,
    pub query_token_count_p95: usize,
    pub query_token_count_max: usize,
    pub descriptor_token_count_p50: usize,
    pub descriptor_token_count_p95: usize,
    pub descriptor_token_count_max: usize,
    pub encoder_forwards_per_query: usize,
    pub attribution_pooled_forwards_per_query: usize,
    pub process_rss_bytes: Option<u64>,
    pub cold_load_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M001Receipt {
    pub schema_version: u16,
    pub protocol: String,
    pub prereg_fingerprint: String,
    pub derived_view_fingerprint: String,
    pub minilm_manifest_hashes: BTreeMap<String, String>,
    pub points: Vec<M001FrontierPoint>,
    pub label_ranks: Vec<M001LabelRank>,
    pub per_tool_maxsim_u64_k16: BTreeMap<String, (usize, usize)>,
    pub unknown_recall_u64_k16: f64,
    pub unknown_lexical_baseline_u64_k16: f64,
    pub masked_recall_u64_k16: f64,
    pub masked_lexical_baseline_u64_k16: f64,
    pub family_recall_u64_k16: BTreeMap<String, BTreeMap<String, f64>>,
    pub current_step_recall_u64_k16: BTreeMap<String, f64>,
    pub explicit_next_step_recall_u64_k16: BTreeMap<String, f64>,
    pub m002_residual_ranks: Vec<M001LabelRank>,
    pub m002_attribution_reproduces_receipt: bool,
    pub resources: M001ResourceEvidence,
    pub empty_queries: usize,
    pub empty_descriptors: usize,
    pub queries_truncated: usize,
    pub descriptors_truncated: usize,
    pub disposition: String,
    pub fingerprint: String,
}

/// Deterministic fingerprint (excludes `fingerprint` and wall-clock).
pub fn receipt_fingerprint_m001(receipt: &M001Receipt) -> Result<String> {
    let mut canonical = serde_json::to_value(receipt).context("serialize receipt")?;
    if let Some(map) = canonical.as_object_mut() {
        map.remove("fingerprint");
        if let Some(resources) = map.get_mut("resources").and_then(|v| v.as_object_mut()) {
            resources.remove("query_encode_p50_ms");
            resources.remove("query_encode_p95_ms");
            resources.remove("maxsim_p50_ms");
            resources.remove("maxsim_p95_ms");
            resources.remove("total_retrieval_p50_ms");
            resources.remove("total_retrieval_p95_ms");
            resources.remove("total_retrieval_max_ms");
            resources.remove("descriptor_precompute_ms");
            resources.remove("cold_load_ms");
            resources.remove("process_rss_bytes");
        }
    }
    let bytes = serde_json::to_vec(&canonical).context("canonical receipt bytes")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

// ---------------------------------------------------------------------------
// Transforms for generalization slices (local mirrors of frozen semantics)
// ---------------------------------------------------------------------------

fn short_hash6(value: &str) -> String {
    hex::encode(&Sha256::digest(value.as_bytes())[..6])
}

/// Mirror of the frozen name-mask transform: every candidate name (and every
/// relevance key) is replaced by `masked_<hash6>`, descriptions untouched.
/// The case id is namespaced (`::masked`) so slice orderings never collide
/// with dev orderings; the suffix is stripped for eligibility lookup. Eval
/// only: masked cases never enter optimizer or calibration input.
fn mask_canonical_names(case: &ToolAdvisorCase) -> ToolAdvisorCase {
    let mut transformed = case.clone();
    transformed.case_id = format!("{}::masked", case.case_id);
    for candidate in &mut transformed.candidates {
        candidate.name = format!("masked_{}", short_hash6(&candidate.name));
        candidate.synthetic_identity = true;
    }
    transformed.relevance = case
        .relevance
        .iter()
        .map(|(name, grade)| (format!("masked_{}", short_hash6(name)), *grade))
        .collect();
    transformed
}

/// Rename map for slice scoring: original tool -> renamed tool under the
/// active transform. Primary mapping aligns candidate multisets through
/// (description, category, disclosure) triples (descriptions are kept, names
/// replaced); relevance-only tools fall back to the transform's own
/// `prefix + hash6(name)` scheme.
fn rename_map(
    original: &ToolAdvisorCase,
    renamed: &ToolAdvisorCase,
    prefix: &str,
) -> BTreeMap<String, String> {
    let mut by_descriptor: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for candidate in &renamed.candidates {
        by_descriptor
            .entry((
                candidate.description.clone(),
                candidate.category.clone(),
                candidate.disclosure.clone(),
            ))
            .or_default()
            .push(candidate.name.clone());
    }
    let mut map = BTreeMap::new();
    for candidate in &original.candidates {
        let key = (
            candidate.description.clone(),
            candidate.category.clone(),
            candidate.disclosure.clone(),
        );
        if let Some(names) = by_descriptor.get_mut(&key) {
            if !names.is_empty() {
                map.insert(candidate.name.clone(), names.remove(0));
            }
        }
    }
    for name in original.relevance.keys() {
        map.entry(name.clone())
            .or_insert_with(|| format!("{prefix}{}", short_hash6(name)));
    }
    map
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

fn percentile(sorted: &mut [f64]) -> (f64, f64) {
    if sorted.is_empty() {
        return (0.0, 0.0);
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f64| sorted[((q * sorted.len() as f64).floor() as usize).min(sorted.len() - 1)];
    (at(0.5), at(0.95))
}

fn percentile_usize(sorted: &mut [usize]) -> (usize, usize, usize) {
    if sorted.is_empty() {
        return (0, 0, 0);
    }
    sorted.sort_unstable();
    let at = |q: f64| sorted[((q * sorted.len() as f64).floor() as usize).min(sorted.len() - 1)];
    (at(0.5), at(0.95), sorted[sorted.len() - 1])
}

fn sample_process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(kb) = line.strip_prefix("VmRSS:") {
                let kb: u64 = kb.split_whitespace().next()?.parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No portable unsafe-free RSS source on this target (same posture
        // as sequence_ranking); latency/cache/forwards carry the evidence.
        None
    }
}

/// Candidate ordering table keyed by (mode, universe, case id).
pub type OrderingTable = BTreeMap<(String, usize, String), Vec<(String, f64)>>;

/// Rank (1-based) of `tool` in `ordered`, or `None` when absent.
fn rank_of(ordered: &[(String, f64)], tool: &str) -> Option<usize> {
    ordered
        .iter()
        .position(|(name, _)| name == tool)
        .map(|index| index + 1)
}

/// Slice recall at u64/K16 over renamed cases: eligibility comes from the
/// original case id (suffix stripped), names through the rename map.
fn slice_recall(
    cases: &[ToolAdvisorCase],
    maps: &BTreeMap<String, BTreeMap<String, String>>,
    mode: &str,
    orderings: &OrderingTable,
    eligible: &BTreeMap<(String, String), bool>,
) -> Result<f64> {
    let mut relevant = 0usize;
    let mut recovered = 0usize;
    for case in cases {
        let original_id = case
            .case_id
            .strip_suffix("::unknown")
            .or_else(|| case.case_id.strip_suffix("::masked"))
            .unwrap_or(&case.case_id);
        let map = maps.get(original_id);
        let inferable: BTreeSet<String> = eligible
            .iter()
            .filter(|((id, _), flag)| id == original_id && **flag)
            .map(|((_, tool), _)| tool.clone())
            .collect();
        for tool in inferable {
            let renamed = map
                .and_then(|m| m.get(&tool))
                .cloned()
                .unwrap_or_else(|| tool.clone());
            relevant += 1;
            let ordered = if mode == UNION_LEXICAL_MODE {
                lexical_ordering(case, UNION_LEXICAL_MODE)?
            } else {
                orderings[&(mode.to_string(), 64, case.case_id.clone())].clone()
            };
            if ordered.iter().take(16).any(|(n, _)| n == &renamed) {
                recovered += 1;
            }
        }
    }
    Ok(if relevant == 0 {
        1.0
    } else {
        recovered as f64 / relevant as f64
    })
}

/// Measure the frozen M001 dev frontier (one run; Commit B).
pub fn measure_m001_frontier(root: &Path) -> Result<M001Receipt> {
    let prereg = load_prereg(root).context("load M001 prereg")?;
    let cases = builtin_cases().context("load corpus")?;
    let view = build_derived_view(&cases).context("derived view")?;
    if view.fingerprint != EXPECTED_DERIVED_VIEW_FINGERPRINT {
        return Err(anyhow!("derived view drifted"));
    }
    let eligible: BTreeMap<(String, String), bool> = view
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.case_id.clone(), entry.candidate.clone()),
                entry.retrieval_eligible,
            )
        })
        .collect();
    let class_of: BTreeMap<(String, String), String> = view
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.case_id.clone(), entry.candidate.clone()),
                format!("{:?}", entry.relevance_class),
            )
        })
        .collect();
    let partition = partition_cases(&cases);
    let dev: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let inferable_for = |case_id: &str| -> BTreeSet<String> {
        eligible
            .iter()
            .filter(|((id, _), flag)| id == case_id && **flag)
            .map(|((_, tool), _)| tool.clone())
            .collect()
    };

    let cold_started = Instant::now();
    let manifest_path = root.join(PREREG_ENCODER_MANIFEST);
    let encoder = CandleBertSequenceEncoder::load(&manifest_path, &Device::Cpu)
        .context("load frozen MiniLM encoder")?;
    let tokenizer_version = format!(
        "{}:{}",
        encoder.assets.manifest.architecture,
        encoder
            .assets
            .manifest
            .hashes
            .get("vocabulary")
            .cloned()
            .unwrap_or_default()
    );
    let weights_hash = encoder
        .assets
        .manifest
        .hashes
        .get("weights")
        .cloned()
        .unwrap_or_default();
    let mut cache = DescriptorTokenCache::new(tokenizer_version, weights_hash);

    // Precompute descriptor token vectors for every deferred candidate text
    // across all universes (static metadata only; never query/context).
    let precompute_started = Instant::now();
    let mut empty_descriptors = 0usize;
    let mut descriptors_truncated = 0usize;
    for universe in FRONTIER_UNIVERSES {
        let expanded = expand_universe_local(&dev, universe).context("expand dev")?;
        for case in &expanded {
            for candidate in &case.candidates {
                if candidate.disclosure != "deferred" {
                    continue;
                }
                let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
                if encoder.tokenizer.tokenize_text(&flat).len() > DESCRIPTOR_TOKEN_CAP {
                    descriptors_truncated += 1;
                }
                let seq = encoder
                    .encode_token_sequence(&flat, DESCRIPTOR_TOKEN_CAP)
                    .context("encode descriptor tokens")?;
                if seq.token_ids.is_empty() {
                    empty_descriptors += 1;
                }
                if seq.vectors.iter().any(|v| v.len() != MINILM_DIM) {
                    return Err(anyhow!("descriptor vector dim drifted"));
                }
                let raw = (seq.token_ids.clone(), seq.vectors.clone());
                cache.get_or_encode(&flat, || Ok(raw))?;
            }
        }
    }
    let descriptor_precompute_ms = precompute_started.elapsed().as_millis();
    // Per-case orderings for both modes at every universe, plus the
    // attribution-only M002-union reproduction at u64.
    let mut orderings: OrderingTable = BTreeMap::new();
    let mut m002_orderings: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut query_encode_ms: Vec<f64> = Vec::new();
    let mut maxsim_ms: Vec<f64> = Vec::new();
    let mut total_ms: Vec<f64> = Vec::new();
    let mut query_token_counts: Vec<usize> = Vec::new();
    let mut empty_queries = 0usize;
    let mut queries_truncated = 0usize;
    let mut pooled_cache: BTreeMap<String, Vec<f32>> = BTreeMap::new();

    for universe in FRONTIER_UNIVERSES {
        let expanded = expand_universe_local(&dev, universe).context("expand dev")?;
        for case in &expanded {
            let turn_started = Instant::now();
            let query_text = RetrievalQueryV2::from_benchmark_context(&case.context).flat_text();
            if encoder.tokenizer.tokenize_text(&query_text).len() > QUERY_TOKEN_CAP {
                queries_truncated += 1;
            }
            let encode_started = Instant::now();
            let query_seq = encoder
                .encode_token_sequence(&query_text, QUERY_TOKEN_CAP)
                .context("encode query tokens")?;
            query_encode_ms.push(encode_started.elapsed().as_secs_f64() * 1000.0);
            query_token_counts.push(query_seq.token_ids.len());
            if query_seq.vectors.iter().any(|v| v.len() != MINILM_DIM) {
                return Err(anyhow!("query vector dim drifted"));
            }
            let query_norm: Vec<Vec<f32>> = query_seq
                .vectors
                .iter()
                .map(|v| normalize_token(v))
                .collect();
            if query_norm.is_empty() {
                empty_queries += 1;
            }
            let deferred: Vec<&super::ToolAdvisorCandidate> = case
                .candidates
                .iter()
                .filter(|c| c.disclosure == "deferred")
                .collect();
            let arith_started = Instant::now();
            let mut maxsim_scored: Vec<(String, f32)> = Vec::with_capacity(deferred.len());
            for candidate in &deferred {
                let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
                let doc =
                    cache.get_or_encode(&flat, || Err(anyhow!("descriptor evaded precompute")))?;
                let score = maxsim_score(&query_norm, &doc).unwrap_or(f32::NEG_INFINITY);
                maxsim_scored.push((candidate.name.clone(), score));
            }
            let maxsim_ordered = order_scored(maxsim_scored);
            maxsim_ms.push(arith_started.elapsed().as_secs_f64() * 1000.0);
            orderings.insert(
                (
                    "exact-maxsim-v1".to_string(),
                    universe,
                    case.case_id.clone(),
                ),
                maxsim_ordered.clone(),
            );
            // Union with the frozen field-weighted lexical scores.
            let lexical = lexical_ordering(case, UNION_LEXICAL_MODE)?;
            let union = fuse_minmax_union(
                &lexical,
                &maxsim_ordered
                    .iter()
                    .map(|(n, s)| (n.clone(), *s))
                    .collect::<Vec<_>>(),
            );
            orderings.insert(
                (
                    "lexical-maxsim-union-v1".to_string(),
                    universe,
                    case.case_id.clone(),
                ),
                union,
            );
            total_ms.push(turn_started.elapsed().as_secs_f64() * 1000.0);

            // Attribution-only M002-union reproduction at u64.
            if universe == 64 {
                let pooled_query = encoder
                    .encode_context(&query_text, super::sequence_encoder::PoolingStrategy::Mean)
                    .context("pooled query (attribution)")?;
                let mut semantic: Vec<(String, f64)> = Vec::with_capacity(deferred.len());
                for candidate in &deferred {
                    let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
                    let emb = if let Some(hit) = pooled_cache.get(&flat) {
                        hit.clone()
                    } else {
                        let e = encoder
                            .encode_with_pooling(
                                "",
                                &flat,
                                super::sequence_encoder::PoolingStrategy::Mean,
                            )
                            .context("pooled descriptor (attribution)")?;
                        pooled_cache.insert(flat.clone(), e.clone());
                        e
                    };
                    semantic.push((candidate.name.clone(), cosine_f64(&pooled_query, &emb)));
                }
                m002_orderings.insert(
                    case.case_id.clone(),
                    fuse_minmax_union(&lexical_ordering(case, UNION_LEXICAL_MODE)?, &semantic),
                );
            }
        }
    }
    let cold_load_ms = cold_started.elapsed().as_millis();

    // Frontier points.
    let mut points = Vec::new();
    for mode in M001_MODES {
        for universe in FRONTIER_UNIVERSES {
            let gate = M001_GATES
                .iter()
                .find(|(size, _)| *size == universe)
                .map(|(_, gate)| *gate)
                .unwrap_or(0.0);
            let expanded = expand_universe_local(&dev, universe).context("expand dev")?;
            for k in FRONTIER_KS {
                let mut relevant = 0usize;
                let mut recovered = 0usize;
                let mut violations = 0usize;
                for case in &expanded {
                    let inferable = inferable_for(&case.case_id);
                    let allowed = eligible_deferred_names_local(case);
                    let ordered = &orderings[&(mode.to_string(), universe, case.case_id.clone())];
                    let top: BTreeSet<String> = ordered
                        .iter()
                        .take(k)
                        .map(|(name, _)| name.clone())
                        .collect();
                    for name in &top {
                        if !allowed.contains(name) {
                            violations += 1;
                        }
                    }
                    for tool in inferable {
                        relevant += 1;
                        if top.contains(&tool) {
                            recovered += 1;
                        }
                    }
                }
                let recall = if relevant == 0 {
                    1.0
                } else {
                    recovered as f64 / relevant as f64
                };
                points.push(M001FrontierPoint {
                    mode: mode.to_string(),
                    universe,
                    k,
                    inferable_relevant: relevant,
                    inferable_recovered: recovered,
                    inferable_recall: recall,
                    violations,
                    passes: recall >= gate && violations == 0,
                });
            }
        }
    }

    // Per-label ranks (attribution table).
    let expanded_u64 = expand_universe_local(&dev, 64).context("expand u64")?;
    let expanded_u128 = expand_universe_local(&dev, 128).context("expand u128")?;
    let expanded_u256 = expand_universe_local(&dev, 256).context("expand u256")?;
    let rank_in = |expanded: &[ToolAdvisorCase],
                   universe: usize,
                   mode: &str,
                   case_id: &str,
                   tool: &str|
     -> Option<usize> {
        expanded
            .iter()
            .find(|c| c.case_id == case_id)
            .and_then(|_| {
                orderings
                    .get(&(mode.to_string(), universe, case_id.to_string()))
                    .and_then(|ordered| rank_of(ordered, tool))
            })
    };
    let mut label_ranks = Vec::new();
    for case in &dev {
        for tool in inferable_for(&case.case_id) {
            let class = class_of
                .get(&(case.case_id.clone(), tool.clone()))
                .cloned()
                .unwrap_or_default();
            label_ranks.push(M001LabelRank {
                case_id: case.case_id.clone(),
                tool: tool.clone(),
                relevance_class: class,
                task_family: case.task_family.clone(),
                maxsim_rank_u64: rank_in(
                    &expanded_u64,
                    64,
                    "exact-maxsim-v1",
                    &case.case_id,
                    &tool,
                ),
                maxsim_rank_u128: rank_in(
                    &expanded_u128,
                    128,
                    "exact-maxsim-v1",
                    &case.case_id,
                    &tool,
                ),
                maxsim_rank_u256: rank_in(
                    &expanded_u256,
                    256,
                    "exact-maxsim-v1",
                    &case.case_id,
                    &tool,
                ),
                union_rank_u64: rank_in(
                    &expanded_u64,
                    64,
                    "lexical-maxsim-union-v1",
                    &case.case_id,
                    &tool,
                ),
                lexical_rank_u64: lexical_ordering(
                    expanded_u64
                        .iter()
                        .find(|c| c.case_id == case.case_id)
                        .expect("case in u64"),
                    UNION_LEXICAL_MODE,
                )
                .ok()
                .and_then(|ordered| rank_of(&ordered, &tool)),
                m002_union_rank_u64: m002_orderings
                    .get(&case.case_id)
                    .and_then(|ordered| rank_of(ordered, &tool)),
            });
        }
    }

    // M002 residual misses: inferable labels the reproduced M002 union
    // misses at u64/K16. The reproduction is valid only if its aggregates
    // match the frozen M002 receipt (51/53 at 64/128... u64 here).
    let mut m002_recovered_u64 = 0usize;
    let mut m002_relevant_u64 = 0usize;
    for case in &expanded_u64 {
        let inferable = inferable_for(&case.case_id);
        let ordered = &m002_orderings[&case.case_id];
        let top: BTreeSet<String> = ordered
            .iter()
            .take(16)
            .map(|(name, _)| name.clone())
            .collect();
        for tool in inferable {
            m002_relevant_u64 += 1;
            if top.contains(&tool) {
                m002_recovered_u64 += 1;
            }
        }
    }
    let m002_attribution_reproduces_receipt = m002_relevant_u64 == 53 && m002_recovered_u64 == 51;
    let m002_residual_ranks: Vec<M001LabelRank> = label_ranks
        .iter()
        .filter(|row| {
            row.m002_union_rank_u64
                .map(|rank| rank > 16)
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    // Per-tool MaxSim recovery at u64/K16.
    let mut per_tool: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for case in &expanded_u64 {
        let ordered = &orderings[&("exact-maxsim-v1".to_string(), 64, case.case_id.clone())];
        let top: BTreeSet<String> = ordered
            .iter()
            .take(16)
            .map(|(name, _)| name.clone())
            .collect();
        for tool in inferable_for(&case.case_id) {
            let entry = per_tool.entry(tool.clone()).or_insert((0, 0));
            entry.0 += 1;
            if top.contains(&tool) {
                entry.1 += 1;
            }
        }
    }

    // Generalization slices at u64/K16 (renamed + masked + lexical baselines
    // on the same transformed universes).
    // NOTE: renamed/masked orderings reuse the main loop machinery below.
    let unknown_cases: Vec<ToolAdvisorCase> = dev
        .iter()
        .map(|c| unknown_tool_holdout(c).unwrap_or_else(|_| c.clone()))
        .collect();
    let masked_cases: Vec<ToolAdvisorCase> = dev.iter().map(mask_canonical_names).collect();
    let unknown_expanded = expand_universe_local(&unknown_cases, 64)?;
    let masked_expanded = expand_universe_local(&masked_cases, 64)?;
    let mut unknown_maps: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (original, renamed) in dev.iter().zip(unknown_cases.iter()) {
        unknown_maps.insert(
            original.case_id.clone(),
            rename_map(original, renamed, "synthetic_"),
        );
    }
    let mut masked_maps: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (original, renamed) in dev.iter().zip(masked_cases.iter()) {
        masked_maps.insert(
            original.case_id.clone(),
            rename_map(original, renamed, "masked_"),
        );
    }
    // Encode renamed/masked slices (same frozen modes; cache keeps growing
    // with static descriptor texts only).
    for case in unknown_expanded.iter().chain(masked_expanded.iter()) {
        let query_text = RetrievalQueryV2::from_benchmark_context(&case.context).flat_text();
        let query_seq = encoder
            .encode_token_sequence(&query_text, QUERY_TOKEN_CAP)
            .context("slice query tokens")?;
        let query_norm: Vec<Vec<f32>> = query_seq
            .vectors
            .iter()
            .map(|v| normalize_token(v))
            .collect();
        let deferred: Vec<&super::ToolAdvisorCandidate> = case
            .candidates
            .iter()
            .filter(|c| c.disclosure == "deferred")
            .collect();
        let mut maxsim_scored = Vec::with_capacity(deferred.len());
        for candidate in &deferred {
            let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
            if encoder.tokenizer.tokenize_text(&flat).len() > DESCRIPTOR_TOKEN_CAP {
                descriptors_truncated += 1;
            }
            let doc = cache.get_or_encode(&flat, || {
                let seq = encoder
                    .encode_token_sequence(&flat, DESCRIPTOR_TOKEN_CAP)
                    .context("slice descriptor tokens")?;
                Ok((seq.token_ids.clone(), seq.vectors.clone()))
            })?;
            maxsim_scored.push((
                candidate.name.clone(),
                maxsim_score(&query_norm, &doc).unwrap_or(f32::NEG_INFINITY),
            ));
        }
        let maxsim_ordered = order_scored(maxsim_scored);
        let lexical = lexical_ordering(case, UNION_LEXICAL_MODE)?;
        orderings.insert(
            ("exact-maxsim-v1".to_string(), 64, case.case_id.clone()),
            maxsim_ordered.clone(),
        );
        orderings.insert(
            (
                "lexical-maxsim-union-v1".to_string(),
                64,
                case.case_id.clone(),
            ),
            fuse_minmax_union(
                &lexical,
                &maxsim_ordered
                    .iter()
                    .map(|(n, s)| (n.clone(), *s))
                    .collect::<Vec<_>>(),
            ),
        );
    }
    let unknown_recall = slice_recall(
        &unknown_expanded,
        &unknown_maps,
        "exact-maxsim-v1",
        &orderings,
        &eligible,
    )?;
    let unknown_lexical = slice_recall(
        &unknown_expanded,
        &unknown_maps,
        UNION_LEXICAL_MODE,
        &orderings,
        &eligible,
    )?;
    let masked_recall = slice_recall(
        &masked_expanded,
        &masked_maps,
        "exact-maxsim-v1",
        &orderings,
        &eligible,
    )?;
    let masked_lexical = slice_recall(
        &masked_expanded,
        &masked_maps,
        UNION_LEXICAL_MODE,
        &orderings,
        &eligible,
    )?;

    // Family + class slices at u64/K16 (MaxSim + lexical + union).
    let mut family_recall: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    {
        let mut by_family: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for case in &dev {
            for tool in inferable_for(&case.case_id) {
                let family = if case.task_family.is_empty() {
                    "unknown".to_string()
                } else {
                    case.task_family.clone()
                };
                by_family
                    .entry(family)
                    .or_default()
                    .push((case.case_id.clone(), tool));
            }
        }
        for (family, labels) in &by_family {
            let mut row = BTreeMap::new();
            for mode in M001_MODES.iter().chain(["field-weighted-lexical"].iter()) {
                let mut relevant = 0usize;
                let mut recovered = 0usize;
                for (case_id, tool) in labels {
                    relevant += 1;
                    let ordered = if *mode == "field-weighted-lexical" {
                        lexical_ordering(
                            expanded_u64
                                .iter()
                                .find(|c| &c.case_id == case_id)
                                .expect("case in u64"),
                            UNION_LEXICAL_MODE,
                        )?
                    } else {
                        orderings[&(mode.to_string(), 64, case_id.clone())].clone()
                    };
                    if ordered.iter().take(16).any(|(n, _)| n == tool) {
                        recovered += 1;
                    }
                }
                row.insert(
                    mode.to_string(),
                    if relevant == 0 {
                        1.0
                    } else {
                        recovered as f64 / relevant as f64
                    },
                );
            }
            family_recall.insert(family.clone(), row);
        }
    }
    let mut class_recall: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for class in ["CurrentStep", "ExplicitNextStep"] {
        let mut row = BTreeMap::new();
        for mode in M001_MODES.iter().chain(["field-weighted-lexical"].iter()) {
            let mut relevant = 0usize;
            let mut recovered = 0usize;
            for entry in &view.entries {
                if !entry.retrieval_eligible {
                    continue;
                }
                if format!("{:?}", entry.relevance_class) != class {
                    continue;
                }
                if !partition
                    .dev_cases
                    .iter()
                    .any(|i| cases[*i].case_id == entry.case_id)
                {
                    continue;
                }
                relevant += 1;
                let case = expanded_u64
                    .iter()
                    .find(|c| c.case_id == entry.case_id)
                    .expect("case in u64");
                let ordered = if *mode == "field-weighted-lexical" {
                    lexical_ordering(case, UNION_LEXICAL_MODE)?
                } else {
                    orderings[&(mode.to_string(), 64, case.case_id.clone())].clone()
                };
                if ordered.iter().take(16).any(|(n, _)| n == &entry.candidate) {
                    recovered += 1;
                }
            }
            row.insert(
                mode.to_string(),
                if relevant == 0 {
                    1.0
                } else {
                    recovered as f64 / relevant as f64
                },
            );
        }
        class_recall.insert(class.to_string(), row);
    }

    // Resources.
    let mut query_encode_sorted = query_encode_ms.clone();
    let (query_p50, query_p95) = percentile(&mut query_encode_sorted);
    let mut maxsim_sorted = maxsim_ms.clone();
    let (maxsim_p50, maxsim_p95) = percentile(&mut maxsim_sorted);
    let mut total_sorted = total_ms.clone();
    let (total_p50, total_p95) = percentile(&mut total_sorted);
    let total_max = total_ms.iter().cloned().fold(0.0f64, f64::max) as u128;
    let mut query_counts = query_token_counts.clone();
    let (q_p50, q_p95, q_max) = percentile_usize(&mut query_counts);
    let mut desc_counts: Vec<usize> = cache.entries.values().map(|(ids, _)| ids.len()).collect();
    let (d_p50, d_p95, d_max) = percentile_usize(&mut desc_counts);
    let resources = M001ResourceEvidence {
        reference_host: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        toolchain: rustc_version_string(),
        descriptor_precompute_ms,
        descriptor_cache_entries: cache.entry_count(),
        descriptor_cache_bytes: cache.bytes(),
        descriptor_cache_bytes_256: cache.bytes(),
        query_encode_p50_ms: query_p50,
        query_encode_p95_ms: query_p95,
        maxsim_p50_ms: maxsim_p50,
        maxsim_p95_ms: maxsim_p95,
        total_retrieval_p50_ms: total_p50,
        total_retrieval_p95_ms: total_p95,
        total_retrieval_max_ms: total_max,
        query_token_count_p50: q_p50,
        query_token_count_p95: q_p95,
        query_token_count_max: q_max,
        descriptor_token_count_p50: d_p50,
        descriptor_token_count_p95: d_p95,
        descriptor_token_count_max: d_max,
        encoder_forwards_per_query: 1,
        attribution_pooled_forwards_per_query: 2,
        process_rss_bytes: sample_process_rss_bytes(),
        cold_load_ms,
    };

    // Disposition per the frozen A/B/D rules.
    let point = |mode: &str, universe: usize, k: usize| -> &M001FrontierPoint {
        points
            .iter()
            .find(|p| p.mode == mode && p.universe == universe && p.k == k)
            .expect("frontier point")
    };
    let clears_gates = |mode: &str| {
        M001_GATES.iter().all(|(universe, gate)| {
            let p = point(mode, *universe, 16);
            p.inferable_recall >= *gate && p.violations == 0
        })
    };
    let disposition = if M001_MODES.iter().any(|m| clears_gates(m)) {
        "A"
    } else {
        // B requires: no authority defect; best mode >=51/53 at u64/u128;
        // >=52/53 at u64/u128 OR a residual miss moved into K16 with at most
        // one new inferable miss; unknown/masked within 0.02 of the matched
        // non-learned baselines; no family regresses >0.03 vs lexical.
        let best_u64 = M001_MODES
            .iter()
            .map(|m| point(m, 64, 16).inferable_recovered)
            .max()
            .unwrap_or(0);
        let best_u128 = M001_MODES
            .iter()
            .map(|m| point(m, 128, 16).inferable_recovered)
            .max()
            .unwrap_or(0);
        let moved_residual = m002_residual_ranks.iter().any(|row| {
            row.maxsim_rank_u64.map(|r| r <= 16).unwrap_or(false)
                || row.union_rank_u64.map(|r| r <= 16).unwrap_or(false)
        });
        let maxsim_misses_beyond_m002 = label_ranks
            .iter()
            .filter(|row| {
                let maxsim_missed = row.maxsim_rank_u64.map(|r| r > 16).unwrap_or(true);
                let m002_hit = row.m002_union_rank_u64.map(|r| r <= 16).unwrap_or(false);
                maxsim_missed && m002_hit
            })
            .count();
        let unknown_ok = unknown_recall + 0.02 >= unknown_lexical;
        let masked_ok = masked_recall + 0.02 >= masked_lexical;
        let family_ok = family_recall.values().all(|row| {
            row.get("exact-maxsim-v1").copied().unwrap_or(0.0) + 0.03
                >= row.get("field-weighted-lexical").copied().unwrap_or(1.0)
                || row.get("lexical-maxsim-union-v1").copied().unwrap_or(0.0) + 0.03
                    >= row.get("field-weighted-lexical").copied().unwrap_or(1.0)
        });
        let violations_clean = points.iter().all(|p| p.violations == 0);
        if violations_clean
            && best_u64 >= 51
            && best_u128 >= 51
            && (best_u64 >= 52
                || best_u128 >= 52
                || (moved_residual && maxsim_misses_beyond_m002 <= 1))
            && unknown_ok
            && masked_ok
            && family_ok
        {
            "B"
        } else {
            "D"
        }
    }
    .to_string();

    let mut receipt = M001Receipt {
        schema_version: 1,
        protocol: "late-interaction-m001-v1".to_string(),
        prereg_fingerprint: prereg.fingerprint.clone(),
        derived_view_fingerprint: EXPECTED_DERIVED_VIEW_FINGERPRINT.to_string(),
        minilm_manifest_hashes: encoder.assets.manifest.hashes.clone(),
        points,
        label_ranks,
        per_tool_maxsim_u64_k16: per_tool,
        unknown_recall_u64_k16: unknown_recall,
        unknown_lexical_baseline_u64_k16: unknown_lexical,
        masked_recall_u64_k16: masked_recall,
        masked_lexical_baseline_u64_k16: masked_lexical,
        family_recall_u64_k16: family_recall,
        current_step_recall_u64_k16: class_recall.get("CurrentStep").cloned().unwrap_or_default(),
        explicit_next_step_recall_u64_k16: class_recall
            .get("ExplicitNextStep")
            .cloned()
            .unwrap_or_default(),
        m002_residual_ranks,
        m002_attribution_reproduces_receipt,
        resources,
        empty_queries,
        empty_descriptors,
        queries_truncated,
        descriptors_truncated,
        disposition,
        fingerprint: String::new(),
    };
    receipt.fingerprint = receipt_fingerprint_m001(&receipt)?;
    Ok(receipt)
}

fn rustc_version_string() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string()
}

pub fn write_receipt_atomic(root: &Path, receipt: &M001Receipt) -> Result<()> {
    let path = root.join(M001_RECEIPT_ASSET);
    let bytes = serde_json::to_vec_pretty(receipt).context("serialize receipt")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).context("write receipt tmp")?;
    std::fs::rename(&tmp, &path).context("publish receipt")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maxsim_hand_worked_fixture() {
        // q0 aligns with d0, q1 aligns with d1; mean of maxima = 1.0.
        let query = vec![vec![1.0f32, 0.0], vec![0.0, 1.0]];
        let descriptor = vec![vec![1.0f32, 0.0], vec![0.0, 1.0], vec![0.5, 0.5]];
        let score = maxsim_score(&query, &descriptor).expect("score");
        assert!((score - 1.0).abs() < 1e-6, "score={score}");
        // Descriptor order cannot change the score.
        let mut shuffled = descriptor.clone();
        shuffled.reverse();
        let shuffled_score = maxsim_score(&query, &shuffled).expect("score");
        assert!((shuffled_score - score).abs() < 1e-6);
        // Empty sides fail closed.
        assert!(maxsim_score(&[], &descriptor).is_none());
        assert!(maxsim_score(&query, &[]).is_none());
    }

    #[test]
    fn maxsim_mean_of_maxima_weights_every_query_token() {
        // q1 has no good match: (1.0 + 0.0) / 2 = 0.5.
        let query = vec![vec![1.0f32, 0.0], vec![0.0, 1.0]];
        let descriptor = vec![vec![1.0f32, 0.0]];
        let score = maxsim_score(&query, &descriptor).expect("score");
        assert!((score - 0.5).abs() < 1e-6, "score={score}");
    }

    #[test]
    fn ordering_tie_break_is_name_ascending() {
        let ordered = order_scored(vec![
            ("beta".to_string(), 0.5),
            ("alpha".to_string(), 0.5),
            ("gamma".to_string(), 0.9),
        ]);
        let names: Vec<&str> = ordered.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["gamma", "alpha", "beta"]);
    }

    #[test]
    fn cache_key_changes_on_text_and_version() {
        let mut cache = DescriptorTokenCache::new("tok".to_string(), "w".to_string());
        let first = cache
            .get_or_encode("read files", || {
                Ok((vec![1, 2], vec![vec![1.0, 0.0], vec![0.0, 1.0]]))
            })
            .expect("encode");
        assert_eq!(first.len(), 2);
        // Normalizes on read.
        assert!((first[0][0] - 1.0).abs() < 1e-6);
        // Hit: encoder must not run again.
        let second = cache
            .get_or_encode("read files", || Err(anyhow!("must not re-encode")))
            .expect("hit");
        assert_eq!(first, second);
        // Different text: miss.
        let third = cache
            .get_or_encode("read files!", || Ok((vec![1, 2, 3], vec![vec![1.0, 0.0]])))
            .expect("miss");
        assert_eq!(third.len(), 1);
        assert_eq!(cache.entry_count(), 2);
        // Deterministic byte accounting: (2*4 + 2*2*4) + (3*4 + 1*2*4).
        assert_eq!(cache.bytes(), 8 + 16 + 12 + 8);
    }

    #[test]
    fn mask_transform_is_deterministic_and_complete() {
        let cases = builtin_cases().expect("corpus");
        let case = cases
            .iter()
            .find(|c| !c.candidates.is_empty())
            .expect("case");
        let first = mask_canonical_names(case);
        let second = mask_canonical_names(case);
        assert_eq!(first.candidates, second.candidates);
        for candidate in &first.candidates {
            assert!(candidate.name.starts_with("masked_"));
            assert!(candidate.synthetic_identity);
        }
        assert_eq!(first.relevance.len(), case.relevance.len());
    }

    #[test]
    fn prereg_receipt_loads_and_binds_contract() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let prereg = super::load_prereg(root).expect("M001 prereg loads");
        assert_eq!(
            prereg.contract_version,
            super::LATE_INTERACTION_CONTRACT_VERSION
        );
        assert_eq!(prereg.query_token_cap, super::QUERY_TOKEN_CAP);
        assert_eq!(prereg.descriptor_token_cap, super::DESCRIPTOR_TOKEN_CAP);
        assert!(prereg.query_token_cap <= 128 && prereg.descriptor_token_cap <= 128);
        assert_eq!(
            prereg.modes,
            super::M001_MODES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    /// Token-count probe for prereg cap selection (train queries + static
    /// descriptors with the real tokenizer; prints distributions, asserts
    /// nothing about dev retrieval).
    #[test]
    #[ignore]
    #[cfg(feature = "tool-advisor-encoder-training")]
    fn probe_token_count_distributions() {
        use super::super::sequence_encoder::WordPieceTokenizer;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let vocab = root.join("target/tool-advisor/reference-assets/all-minilm-l6-v2/vocab.txt");
        let tokenizer = WordPieceTokenizer::from_vocab(&vocab).expect("vocab");
        let cases = builtin_cases().expect("corpus");
        let partition = partition_cases(&cases);
        let mut train_query: Vec<usize> = Vec::new();
        for index in &partition.train_cases {
            let query =
                RetrievalQueryV2::from_benchmark_context(&cases[*index].context).flat_text();
            train_query.push(tokenizer.tokenize_text(&query).len());
        }
        let mut descriptor_counts: Vec<usize> = Vec::new();
        for universe in FRONTIER_UNIVERSES {
            let expanded = expand_universe_local(&cases, universe).expect("expand corpus");
            for case in &expanded {
                for candidate in &case.candidates {
                    if candidate.disclosure != "deferred" {
                        continue;
                    }
                    let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
                    descriptor_counts.push(tokenizer.tokenize_text(&flat).len());
                }
            }
        }
        descriptor_counts.sort_unstable();
        descriptor_counts.dedup();
        let mut train_sorted = train_query.clone();
        train_sorted.sort_unstable();
        let at = |sorted: &[usize], q: f64| {
            sorted[((q * sorted.len() as f64).floor() as usize).min(sorted.len() - 1)]
        };
        eprintln!(
            "train queries n={} p50={} p95={} p100={} max-lines-ok",
            train_sorted.len(),
            at(&train_sorted, 0.5),
            at(&train_sorted, 0.95),
            at(&train_sorted, 1.0)
        );
        let mut all_desc: Vec<usize> = Vec::new();
        for universe in FRONTIER_UNIVERSES {
            let expanded = expand_universe_local(&cases, universe).expect("expand corpus");
            for case in &expanded {
                for candidate in &case.candidates {
                    if candidate.disclosure != "deferred" {
                        continue;
                    }
                    let flat = RetrievalDescriptorV2::from_candidate(candidate, None).flat_text();
                    all_desc.push(tokenizer.tokenize_text(&flat).len());
                }
            }
        }
        all_desc.sort_unstable();
        eprintln!(
            "descriptors n={} p50={} p95={} p100={}",
            all_desc.len(),
            at(&all_desc, 0.5),
            at(&all_desc, 0.95),
            at(&all_desc, 1.0)
        );
        eprintln!("unique descriptor token counts: {descriptor_counts:?}");
    }

    /// Generate the committed M001 receipt (encoder-training gate, ignored).
    /// Run explicitly AFTER the preregistration commit, exactly once.
    #[test]
    #[ignore]
    #[cfg(feature = "tool-advisor-encoder-training")]
    fn generate_m001_receipt() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let receipt = super::measure_m001_frontier(root).expect("M001 frontier");
        eprintln!(
            "M001 disposition={} attribution_reproduces_m002={}",
            receipt.disposition, receipt.m002_attribution_reproduces_receipt
        );
        for point in &receipt.points {
            eprintln!(
                "{} u{} k{}: {}/{} = {:.4} violations={} passes={}",
                point.mode,
                point.universe,
                point.k,
                point.inferable_recovered,
                point.inferable_relevant,
                point.inferable_recall,
                point.violations,
                point.passes
            );
        }
        eprintln!(
            "unknown maxsim={:.4} lexical={:.4} | masked maxsim={:.4} lexical={:.4}",
            receipt.unknown_recall_u64_k16,
            receipt.unknown_lexical_baseline_u64_k16,
            receipt.masked_recall_u64_k16,
            receipt.masked_lexical_baseline_u64_k16
        );
        super::write_receipt_atomic(root, &receipt).expect("write receipt");
    }
}
