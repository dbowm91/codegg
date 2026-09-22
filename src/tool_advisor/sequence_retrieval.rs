//! Deterministic hybrid coarse retrieval for the sequence-encoder experiment.
//!
//! The retriever is deliberately downstream of the resolved surface: callers
//! pass only already-authorized deferred descriptors.  Its cache contains
//! descriptor embeddings, never user context.  Any encoder failure returns the
//! BM25 ordering so retrieval cannot become an authority or availability gate.

use super::context_v2::AdvisorContextV2;
use super::sequence_encoder::{CandleBertSequenceEncoder, PoolingStrategy};
use super::{baseline_prediction, ToolAdvisorCandidate, ToolAdvisorCase};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Instant;

pub const RETRIEVAL_SCHEMA_VERSION: u16 = 1;
pub const RRF_K: f64 = 60.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RetrievalMode {
    Bm25,
    Semantic,
    Rrf,
    NormalizedUnion,
}

impl RetrievalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::Semantic => "semantic",
            Self::Rrf => "rrf",
            Self::NormalizedUnion => "normalized-union",
        }
    }
}

pub fn parse_mode(value: &str) -> Result<RetrievalMode> {
    match value {
        "bm25" => Ok(RetrievalMode::Bm25),
        "semantic" => Ok(RetrievalMode::Semantic),
        "rrf" => Ok(RetrievalMode::Rrf),
        "normalized-union" => Ok(RetrievalMode::NormalizedUnion),
        other => Err(anyhow!("unsupported retrieval mode {other}")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalCacheKey {
    pub surface_fingerprint: String,
    pub canonical_name: String,
    pub normalized_descriptor_hash: String,
    pub encoder_tokenizer_version: String,
}

#[derive(Debug, Default)]
pub struct DescriptorEmbeddingCache {
    entries: BTreeMap<RetrievalCacheKey, Vec<f32>>,
    surface_fingerprint: Option<String>,
}

impl DescriptorEmbeddingCache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear_for_surface(&mut self, surface_fingerprint: &str) {
        if self.surface_fingerprint.as_deref() != Some(surface_fingerprint) {
            self.entries.clear();
            self.surface_fingerprint = Some(surface_fingerprint.to_string());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub schema_version: u16,
    pub mode: String,
    pub k: usize,
    pub names: Vec<String>,
    pub scores: Vec<f64>,
    pub eligible_count: usize,
    pub dropped_count: usize,
    pub query_encoding_ms: u128,
    pub retrieval_ms: u128,
    pub cache_entries: usize,
    pub fallback_to_bm25: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalFrontierPoint {
    pub mode: String,
    pub k: usize,
    pub cases: usize,
    pub relevant_tools: usize,
    pub recovered_tools: usize,
    pub recall: f64,
    pub mean_latency_ms: f64,
    pub max_latency_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalFrontierReport {
    pub schema_version: u16,
    pub surface_fingerprint: String,
    pub points: Vec<RetrievalFrontierPoint>,
    pub cache_warm_entries: usize,
    pub cache_contains_context: bool,
}

fn descriptor(candidate: &ToolAdvisorCandidate) -> String {
    format!(
        "canonical name: {}; description: {}; category: {}; disclosure: {}",
        candidate.name, candidate.description, candidate.category, candidate.disclosure
    )
}

fn normalized_descriptor(candidate: &ToolAdvisorCandidate) -> String {
    descriptor(candidate)
        .split_whitespace()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn cosine(left: &[f32], right: &[f32]) -> f64 {
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

fn eligible(case: &ToolAdvisorCase) -> Vec<&ToolAdvisorCandidate> {
    case.candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .collect()
}

fn bm25_order(case: &ToolAdvisorCase, k: usize) -> Vec<(String, f64)> {
    let prediction = baseline_prediction(case, crate::tool::catalog::SearchMode::BM25);
    let allowed = eligible(case)
        .into_iter()
        .map(|candidate| candidate.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut ranked = prediction
        .ranked
        .into_iter()
        .filter(|candidate| allowed.contains(&candidate.name))
        .map(|candidate| (candidate.name, candidate.score))
        .collect::<Vec<_>>();
    let known = ranked
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for candidate in eligible(case) {
        if !known.contains(&candidate.name) {
            ranked.push((candidate.name.clone(), 0.0));
        }
    }
    ranked.sort_by(|left, right| left.0.cmp(&right.0));
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.truncate(k);
    ranked
}

pub struct HybridRetriever<'a> {
    pub encoder: &'a CandleBertSequenceEncoder,
    pub cache: DescriptorEmbeddingCache,
    pub encoder_tokenizer_version: String,
}

impl<'a> HybridRetriever<'a> {
    pub fn new(encoder: &'a CandleBertSequenceEncoder) -> Self {
        Self {
            encoder,
            cache: DescriptorEmbeddingCache::default(),
            encoder_tokenizer_version: format!(
                "{}:{}",
                encoder.assets.manifest.architecture,
                encoder
                    .assets
                    .manifest
                    .hashes
                    .get("vocabulary")
                    .cloned()
                    .unwrap_or_default()
            ),
        }
    }

    pub fn retrieve(
        &mut self,
        case: &ToolAdvisorCase,
        surface_fingerprint: &str,
        mode: RetrievalMode,
        k: usize,
    ) -> Result<RetrievalResult> {
        if k == 0 {
            return Err(anyhow!("retrieval K must be positive"));
        }
        self.cache.clear_for_surface(surface_fingerprint);
        let started = Instant::now();
        let eligible = eligible(case);
        let eligible_count = eligible.len();
        if eligible.is_empty() {
            return Ok(RetrievalResult {
                schema_version: RETRIEVAL_SCHEMA_VERSION,
                mode: mode.as_str().into(),
                k,
                names: Vec::new(),
                scores: Vec::new(),
                eligible_count,
                dropped_count: 0,
                query_encoding_ms: 0,
                retrieval_ms: started.elapsed().as_millis(),
                cache_entries: self.cache.len(),
                fallback_to_bm25: false,
            });
        }
        let query_started = Instant::now();
        let context = AdvisorContextV2::from_benchmark_context(&case.context).serialize();
        let query = if matches!(
            mode,
            RetrievalMode::Semantic | RetrievalMode::Rrf | RetrievalMode::NormalizedUnion
        ) {
            Some(
                self.encoder
                    .encode_context(&context, PoolingStrategy::Mean)?,
            )
        } else {
            None
        };
        let query_encoding_ms = query_started.elapsed().as_millis();
        let mut semantic = Vec::with_capacity(eligible.len());
        if let Some(query) = query.as_deref() {
            for candidate in &eligible {
                let key = RetrievalCacheKey {
                    surface_fingerprint: surface_fingerprint.to_string(),
                    canonical_name: candidate.name.clone(),
                    normalized_descriptor_hash: hex::encode(Sha256::digest(
                        normalized_descriptor(candidate).as_bytes(),
                    )),
                    encoder_tokenizer_version: self.encoder_tokenizer_version.clone(),
                };
                let embedding = match self.cache.entries.get(&key) {
                    Some(embedding) => embedding.clone(),
                    None => {
                        let embedding = self.encoder.encode_with_pooling(
                            "",
                            &descriptor(candidate),
                            PoolingStrategy::Mean,
                        )?;
                        self.cache.entries.insert(key, embedding.clone());
                        embedding
                    }
                };
                semantic.push((candidate.name.clone(), cosine(query, &embedding)));
            }
        }
        let bm25 = bm25_order(case, eligible_count.max(k));
        let mut scores = match mode {
            RetrievalMode::Bm25 => bm25,
            RetrievalMode::Semantic => semantic,
            RetrievalMode::Rrf => fuse_rrf(&bm25, &semantic),
            RetrievalMode::NormalizedUnion => fuse_normalized(&bm25, &semantic),
        };
        scores.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        scores.truncate(k.min(eligible_count));
        Ok(RetrievalResult {
            schema_version: RETRIEVAL_SCHEMA_VERSION,
            mode: mode.as_str().into(),
            k,
            names: scores.iter().map(|(name, _)| name.clone()).collect(),
            scores: scores.iter().map(|(_, score)| *score).collect(),
            eligible_count,
            dropped_count: eligible_count.saturating_sub(scores.len()),
            query_encoding_ms,
            retrieval_ms: started.elapsed().as_millis(),
            cache_entries: self.cache.len(),
            fallback_to_bm25: false,
        })
    }

    pub fn retrieve_with_fallback(
        &mut self,
        case: &ToolAdvisorCase,
        surface_fingerprint: &str,
        mode: RetrievalMode,
        k: usize,
    ) -> RetrievalResult {
        match self.retrieve(case, surface_fingerprint, mode, k) {
            Ok(result) => result,
            Err(_) => {
                let names = bm25_order(case, k);
                RetrievalResult {
                    schema_version: RETRIEVAL_SCHEMA_VERSION,
                    mode: mode.as_str().into(),
                    k,
                    scores: names.iter().map(|(_, score)| *score).collect(),
                    names: names.into_iter().map(|(name, _)| name).collect(),
                    eligible_count: eligible(case).len(),
                    dropped_count: 0,
                    query_encoding_ms: 0,
                    retrieval_ms: 0,
                    cache_entries: self.cache.len(),
                    fallback_to_bm25: true,
                }
            }
        }
    }
}

fn fuse_rrf(bm25: &[(String, f64)], semantic: &[(String, f64)]) -> Vec<(String, f64)> {
    let mut scores = BTreeMap::new();
    for (rank, (name, _)) in bm25.iter().enumerate() {
        *scores.entry(name.clone()).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
    }
    for (rank, (name, _)) in ranked(semantic).iter().enumerate() {
        *scores.entry(name.clone()).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
    }
    scores.into_iter().collect()
}

fn fuse_normalized(bm25: &[(String, f64)], semantic: &[(String, f64)]) -> Vec<(String, f64)> {
    let bm25 = normalize(bm25);
    let semantic = normalize(semantic);
    let mut scores = BTreeMap::new();
    for (name, score) in bm25.into_iter().chain(semantic) {
        *scores.entry(name).or_insert(0.0) += score;
    }
    scores.into_iter().collect()
}

fn ranked(values: &[(String, f64)]) -> Vec<(String, f64)> {
    let mut values = values.to_vec();
    values.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    values
}

fn normalize(values: &[(String, f64)]) -> Vec<(String, f64)> {
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

pub fn frontier(
    retriever: &mut HybridRetriever<'_>,
    cases: &[ToolAdvisorCase],
    surface_fingerprint: &str,
    ks: &[usize],
    modes: &[RetrievalMode],
) -> Result<RetrievalFrontierReport> {
    let mut points = Vec::new();
    for mode in modes {
        for k in ks {
            let mut relevant = 0usize;
            let mut recovered = 0usize;
            let mut total_latency = 0u128;
            let mut max_latency = 0u128;
            for case in cases {
                let expected = case
                    .relevance
                    .keys()
                    .filter(|name| {
                        eligible(case)
                            .iter()
                            .any(|candidate| &candidate.name == *name)
                    })
                    .count();
                if expected == 0 {
                    continue;
                }
                let started = Instant::now();
                let result = retriever.retrieve_with_fallback(case, surface_fingerprint, *mode, *k);
                let latency = started.elapsed().as_millis();
                total_latency += latency;
                max_latency = max_latency.max(latency);
                relevant += expected;
                recovered += case
                    .relevance
                    .keys()
                    .filter(|name| result.names.iter().any(|candidate| candidate == *name))
                    .count();
            }
            points.push(RetrievalFrontierPoint {
                mode: mode.as_str().into(),
                k: *k,
                cases: cases.len(),
                relevant_tools: relevant,
                recovered_tools: recovered,
                recall: if relevant == 0 {
                    0.0
                } else {
                    recovered as f64 / relevant as f64
                },
                mean_latency_ms: if cases.is_empty() {
                    0.0
                } else {
                    total_latency as f64 / cases.len() as f64
                },
                max_latency_ms: max_latency,
            });
        }
    }
    Ok(RetrievalFrontierReport {
        schema_version: RETRIEVAL_SCHEMA_VERSION,
        surface_fingerprint: surface_fingerprint.into(),
        points,
        cache_warm_entries: retriever.cache.len(),
        cache_contains_context: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn candidate(name: &str, disclosure: &str) -> ToolAdvisorCandidate {
        ToolAdvisorCandidate {
            name: name.into(),
            description: format!("{name} descriptor"),
            category: "ReadOnly".into(),
            disclosure: disclosure.into(),
            synthetic_identity: false,
        }
    }

    fn case() -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: super::super::CASE_SCHEMA_VERSION,
            case_id: "retrieval".into(),
            context: "inspect files".into(),
            candidates: vec![candidate("hidden", "deferred"), candidate("core", "core")],
            relevance: BTreeMap::from([(String::from("hidden"), 3)]),
            preferred_order: vec!["hidden".into()],
            none: false,
            tags: vec![],
            group_id: "retrieval".into(),
            provenance: "test".into(),
            semantic_group: "retrieval".into(),
            leakage_group: String::new(),
            task_family: "filesystem".into(),
            tool_family: "filesystem".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn cache_invalidates_on_surface_change() {
        let mut cache = DescriptorEmbeddingCache {
            surface_fingerprint: Some("one".into()),
            ..Default::default()
        };
        cache.entries.insert(
            RetrievalCacheKey {
                surface_fingerprint: "one".into(),
                canonical_name: "tool".into(),
                normalized_descriptor_hash: "hash".into(),
                encoder_tokenizer_version: "encoder".into(),
            },
            vec![1.0],
        );
        cache.clear_for_surface("two");
        assert_eq!(cache.len(), 0);
        assert!(eligible(&case())
            .iter()
            .all(|candidate| candidate.disclosure == "deferred"));
    }

    #[test]
    fn authority_filter_excludes_core_candidates() {
        let names = bm25_order(&case(), 16);
        assert_eq!(
            names
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["hidden"]
        );
    }

    #[test]
    fn fusion_ties_are_deterministic() {
        assert_eq!(
            fuse_rrf(
                &[("b".into(), 1.0), ("a".into(), 1.0)],
                &[("a".into(), 0.5), ("b".into(), 0.5)]
            ),
            vec![
                ("a".into(), 0.03252247488101534),
                ("b".into(), 0.03252247488101534)
            ]
        );
    }
}
