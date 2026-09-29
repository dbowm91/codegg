//! M002 deterministic Retrieval Signal V2 frontier experiment.
//!
//! This module is test-only infrastructure behind the encoder-training
//! feature. It never changes catalog ranking, disclosure, or runtime
//! selection. All measured candidates must already be in the expanded
//! deferred universe.

use super::context_v2::AdvisorContextV2;
use super::operating_point::expand_universe;
use super::retrieval_signal::{
    descriptor_fingerprint_v2, extract_schema_fields, live_builtin_schema,
    m007_preregistration_spec, signal_tokens, RetrievalCacheKeyV2, RetrievalDescriptorV2,
    RetrievalQueryV2, DETERMINISTIC_LEXICAL_VARIANTS, DETERMINISTIC_POOLING_VARIANTS,
    DETERMINISTIC_SEMANTIC_VARIANTS, GATE_RECALL_128, GATE_RECALL_256, GATE_RECALL_64,
    M004_DEV_CASES, PREREG_PRIMARY_KS, PREREG_UNIVERSES, RETRIEVAL_SIGNAL_SCHEMA_VERSION,
};
use super::sequence_encoder::{CandleBertSequenceEncoder, PoolingStrategy};
use super::{
    dataset_fingerprint, load_cases, partition_cases, ToolAdvisorCandidate, ToolAdvisorCase,
};
use crate::tool::catalog::SearchMode;
use anyhow::{anyhow, Context, Result};
use candle_core::Device;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const M002_PROTOCOL: &str = "retrieval-signal-m002-frontier-v1";
pub const M002_RECEIPT_PATH: &str = "assets/tool-advisor/retrieval-signal-m002-frontier.json";
pub const M002_OUTPUT_DIR: &str = "target/tool-advisor/retrieval-signal-m002";
pub const M002_ENCODER_MANIFEST: &str =
    "target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json";
const M006_EXCLUDED: [(&str, &str); 3] = [
    ("filesystem-semantic-013-variant-1", "table_filter"),
    ("verification-semantic-105-variant-1", "write"),
    ("research-semantic-125-variant-1", "lsp_rename"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SliceRecall {
    pub relevant: usize,
    pub recovered: usize,
    pub recall: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M002FrontierPoint {
    pub arm: String,
    pub universe: usize,
    pub k: usize,
    pub cases: usize,
    pub eligible_relevant: usize,
    pub recovered_relevant: usize,
    pub recall: f64,
    pub primary_relevant: usize,
    pub primary_recovered: usize,
    pub primary_recall: f64,
    pub secondary_relevant: usize,
    pub secondary_recovered: usize,
    pub secondary_recall: f64,
    pub per_tool: BTreeMap<String, SliceRecall>,
    pub builtin_vs_synthetic: BTreeMap<String, SliceRecall>,
    pub schema_presence: BTreeMap<String, SliceRecall>,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_max_ms: f64,
    pub descriptor_cache_entries: usize,
    pub descriptor_cache_bytes: usize,
    pub authority_violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncoderCostReport {
    pub model_load_ms: f64,
    pub first_query_encode_ms: f64,
    pub first_descriptor_encode_ms: f64,
    pub warm_query_encode_p50_ms: f64,
    pub warm_descriptor_encode_p50_ms: f64,
    pub warm_repetitions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentMissResult {
    pub tool: String,
    pub case_id: String,
    pub prior_bm25_rank: usize,
    pub m002_rank_by_arm: BTreeMap<String, usize>,
    pub entered_k: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M002SweepReceipt {
    pub schema_version: u16,
    pub protocol: String,
    pub preregistration_sha256: String,
    pub m006_decision_sha256: String,
    pub dataset_sha256: String,
    pub dev_partition_sha256: String,
    pub universe_fingerprint: String,
    pub encoder_manifest_sha256: String,
    pub encoder_costs: EncoderCostReport,
    pub cases: usize,
    pub eligible_relevant_by_universe: BTreeMap<usize, usize>,
    pub required_hits: BTreeMap<usize, usize>,
    pub frontier: Vec<M002FrontierPoint>,
    pub persistent_misses: Vec<PersistentMissResult>,
    pub authority_violations: usize,
    pub selected_arm: Option<String>,
    pub selected_k: Option<usize>,
    pub disposition: String,
}

#[derive(Debug, Clone)]
struct EvaluatedCase {
    case: ToolAdvisorCase,
    ranked: Vec<(String, f64)>,
    latency_ms: f64,
    cache_entries: usize,
    cache_bytes: usize,
}

#[derive(Debug, Default)]
struct DescriptorCache {
    entries: BTreeMap<RetrievalCacheKeyV2, Vec<f32>>,
}

impl DescriptorCache {
    fn bytes(&self) -> usize {
        self.entries
            .values()
            .map(|embedding| embedding.len() * 4)
            .sum()
    }
}

fn catalog_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn candidate_descriptor(candidate: &ToolAdvisorCandidate) -> RetrievalDescriptorV2 {
    let schema = live_builtin_schema(&candidate.name)
        .map(|schema| extract_schema_fields(&schema))
        .unwrap_or_default();
    RetrievalDescriptorV2::from_candidate(candidate, schema)
}

pub(super) fn m002_context(case: &ToolAdvisorCase) -> AdvisorContextV2 {
    AdvisorContextV2::from_benchmark_context(&case.context)
}

fn deferred_candidates(case: &ToolAdvisorCase) -> Vec<&ToolAdvisorCandidate> {
    case.candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .collect()
}

fn sorted_rank(scores: Vec<(String, f64)>) -> Vec<(String, f64)> {
    let mut scores = scores;
    scores.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    scores
}

fn bm25_scores(query: &[String], documents: &[(String, Vec<String>)]) -> Vec<(String, f64)> {
    let n = documents.len() as f64;
    if documents.is_empty() {
        return Vec::new();
    }
    let mut document_frequency = BTreeMap::<String, usize>::new();
    let mut total_length = 0usize;
    for (_, tokens) in documents {
        total_length += tokens.len();
        for token in tokens.iter().collect::<BTreeSet<_>>() {
            *document_frequency.entry(token.clone()).or_default() += 1;
        }
    }
    let average_length = (total_length as f64 / n).max(1.0);
    let mut query_frequency = HashMap::<String, usize>::new();
    for token in query {
        *query_frequency.entry(token.clone()).or_default() += 1;
    }
    documents
        .iter()
        .map(|(name, tokens)| {
            let mut tf = HashMap::<&str, usize>::new();
            for token in tokens {
                *tf.entry(token).or_default() += 1;
            }
            let doc_len = tokens.len() as f64;
            let mut score = 0.0;
            for (term, qtf) in &query_frequency {
                let term_tf = tf.get(term.as_str()).copied().unwrap_or(0) as f64;
                if term_tf == 0.0 {
                    continue;
                }
                let df = document_frequency.get(term).copied().unwrap_or(0) as f64;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                let denominator = term_tf
                    + super::retrieval_signal::M007_BM25_K1
                        * (1.0 - super::retrieval_signal::M007_BM25_B
                            + super::retrieval_signal::M007_BM25_B * doc_len / average_length);
                score +=
                    idf * (*qtf as f64) * term_tf * (super::retrieval_signal::M007_BM25_K1 + 1.0)
                        / denominator;
            }
            (name.clone(), score)
        })
        .collect()
}

fn field_texts(descriptor: &RetrievalDescriptorV2) -> Vec<String> {
    let schema = descriptor
        .schema_fields
        .iter()
        .map(|field| {
            format!(
                "{} {} {} {}",
                field.name,
                field.field_type,
                field.description,
                if field.required {
                    "required"
                } else {
                    "optional"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        descriptor.canonical_name.clone(),
        descriptor.identifier_tokens.join(" "),
        descriptor.description.clone(),
        format!("{} {}", descriptor.category, descriptor.disclosure),
        schema,
        String::new(), // normalization transform has preregistered weight zero
    ]
}

fn query_field_texts(context: &AdvisorContextV2) -> Vec<String> {
    vec![
        context.current_objective.clone().unwrap_or_default(),
        context.current_task.clone().unwrap_or_default(),
        context.next_steps.join(" "),
        context.unresolved_signal.clone().unwrap_or_default(),
        context.capability_cue.clone().unwrap_or_default(),
    ]
}

fn field_weighted_bm25(
    query_fields: &[Vec<String>],
    documents: &[(String, Vec<Vec<String>>)],
) -> Vec<(String, f64)> {
    let n = documents.len() as f64;
    if n == 0.0 {
        return Vec::new();
    }
    let mut df = BTreeMap::<String, usize>::new();
    let mut average_lengths =
        vec![0.0; super::retrieval_signal::M007_FIELD_WEIGHT_DESCRIPTOR.len()];
    for (_, fields) in documents {
        let mut document_terms = BTreeSet::new();
        for (index, tokens) in fields.iter().enumerate() {
            average_lengths[index] += tokens.len() as f64 / n;
            document_terms.extend(tokens.iter().cloned());
        }
        for term in document_terms {
            *df.entry(term).or_default() += 1;
        }
    }
    let mut qtf = BTreeMap::<String, f64>::new();
    for (field_index, tokens) in query_fields.iter().enumerate() {
        let weight = super::retrieval_signal::M007_FIELD_WEIGHT_QUERY[field_index].1;
        for token in tokens {
            *qtf.entry(token.clone()).or_default() += weight;
        }
    }
    documents
        .iter()
        .map(|(name, fields)| {
            let mut score = 0.0;
            for (term, weighted_query_tf) in &qtf {
                let mut weighted_tf = 0.0;
                for (field_index, tokens) in fields.iter().enumerate() {
                    let field_weight =
                        super::retrieval_signal::M007_FIELD_WEIGHT_DESCRIPTOR[field_index].1;
                    let tf = tokens.iter().filter(|token| *token == term).count() as f64;
                    let average = average_lengths[field_index];
                    if field_weight == 0.0 || tf == 0.0 || average == 0.0 {
                        continue;
                    }
                    let length = tokens.len() as f64;
                    let denominator = 1.0 - super::retrieval_signal::M007_BM25_B
                        + super::retrieval_signal::M007_BM25_B * length / average;
                    weighted_tf += field_weight * tf / denominator;
                }
                if weighted_tf == 0.0 {
                    continue;
                }
                let term_df = df.get(term).copied().unwrap_or(0) as f64;
                let idf = ((n - term_df + 0.5) / (term_df + 0.5) + 1.0).ln();
                score += idf
                    * weighted_query_tf
                    * (super::retrieval_signal::M007_BM25_K1 + 1.0)
                    * weighted_tf
                    / (super::retrieval_signal::M007_BM25_K1 + weighted_tf);
            }
            (name.clone(), score)
        })
        .collect()
}

fn lexical_rank(case: &ToolAdvisorCase, arm: &str) -> Result<Vec<(String, f64)>> {
    let allowed: BTreeSet<String> = deferred_candidates(case)
        .into_iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    if arm == "v1-bm25-baseline" {
        let prediction = super::baseline_prediction(case, SearchMode::BM25);
        let mut scores: BTreeMap<String, f64> = prediction
            .ranked
            .into_iter()
            .filter(|item| allowed.contains(&item.name))
            .map(|item| (item.name, item.score))
            .collect();
        for name in &allowed {
            scores.entry(name.clone()).or_insert(0.0);
        }
        return Ok(sorted_rank(scores.into_iter().collect()));
    }
    let context = m002_context(case);
    let query = RetrievalQueryV2::from_advisor_context(&context);
    let candidates = deferred_candidates(case);
    let descriptors: Vec<_> = candidates
        .iter()
        .map(|candidate| (candidate.name.clone(), candidate_descriptor(candidate)))
        .collect();
    let scores = match arm {
        "signal-v2-flat-bm25" => {
            let documents = descriptors
                .iter()
                .map(|(name, descriptor)| (name.clone(), catalog_tokens(&descriptor.flat_text())))
                .collect::<Vec<_>>();
            bm25_scores(&catalog_tokens(&query.flat_text()), &documents)
        }
        "signal-v2-field-weighted-bm25" => {
            let documents = descriptors
                .iter()
                .map(|(name, descriptor)| {
                    (
                        name.clone(),
                        field_texts(descriptor)
                            .iter()
                            .map(|field| catalog_tokens(field))
                            .collect(),
                    )
                })
                .collect::<Vec<_>>();
            let qfields = query_field_texts(&context)
                .iter()
                .map(|field| catalog_tokens(field))
                .collect::<Vec<_>>();
            field_weighted_bm25(&qfields, &documents)
        }
        "signal-v2-normalized-bm25" => {
            let documents = descriptors
                .iter()
                .map(|(name, descriptor)| (name.clone(), signal_tokens(&descriptor.flat_text())))
                .collect::<Vec<_>>();
            bm25_scores(&signal_tokens(&query.flat_text()), &documents)
        }
        _ => return Err(anyhow!("unknown M002 lexical arm {arm}")),
    };
    Ok(sorted_rank(scores))
}

fn pooling(value: &str) -> Result<PoolingStrategy> {
    match value {
        "mean" => Ok(PoolingStrategy::Mean),
        "cls" => Ok(PoolingStrategy::Cls),
        _ => Err(anyhow!("unknown M002 pooling strategy {value}")),
    }
}

fn unit_norm(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if norm > f64::EPSILON {
        for value in &mut vector {
            *value = (f64::from(*value) / norm) as f32;
        }
    }
    vector
}

fn surface_fingerprint(case: &ToolAdvisorCase) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"retrieval-signal-m002-surface-v1\n");
    let mut descriptors = deferred_candidates(case)
        .into_iter()
        .map(|candidate| {
            let descriptor = candidate_descriptor(candidate);
            (
                candidate.name.clone(),
                candidate.disclosure.clone(),
                descriptor_fingerprint_v2(&descriptor),
            )
        })
        .collect::<Vec<_>>();
    descriptors.sort();
    for (name, disclosure, descriptor_fingerprint) in descriptors {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(disclosure.as_bytes());
        hasher.update(b"\0");
        hasher.update(descriptor_fingerprint.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

pub(super) fn semantic_texts(
    case: &ToolAdvisorCase,
    arm: &str,
) -> Result<(String, Vec<(String, RetrievalDescriptorV2, String)>)> {
    let query = RetrievalQueryV2::from_advisor_context(&m002_context(case));
    let query_text = match arm {
        "signal-v2-flat" | "signal-v2-field-labelled-descriptor" => query.flat_text(),
        "signal-v2-field-labelled-query" => query.serialize(),
        _ => return Err(anyhow!("unknown M002 semantic arm {arm}")),
    };
    let descriptors = deferred_candidates(case)
        .into_iter()
        .map(|candidate| {
            let descriptor = candidate_descriptor(candidate);
            let text = match arm {
                "signal-v2-flat" | "signal-v2-field-labelled-query" => descriptor.flat_text(),
                "signal-v2-field-labelled-descriptor" => descriptor.field_labelled_text(),
                _ => unreachable!(),
            };
            (candidate.name.clone(), descriptor, text)
        })
        .collect();
    Ok((query_text, descriptors))
}

fn semantic_rank(
    case: &ToolAdvisorCase,
    arm: &str,
    pool: &str,
    encoder: &CandleBertSequenceEncoder,
    encoder_version: &str,
    cache: &mut DescriptorCache,
) -> Result<Vec<(String, f64)>> {
    let strategy = pooling(pool)?;
    let (query_text, descriptors) = semantic_texts(case, arm)?;
    let query_embedding = unit_norm(encoder.encode_context(&query_text, strategy)?);
    let surface = surface_fingerprint(case);
    let mut scores = Vec::with_capacity(descriptors.len());
    for (name, descriptor, text) in descriptors {
        let key = RetrievalCacheKeyV2 {
            representation_schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
            semantic_arm: arm.to_string(),
            descriptor_fingerprint: descriptor_fingerprint_v2(&descriptor),
            encoder_tokenizer_version: encoder_version.to_string(),
            pooling: pool.to_string(),
            surface_fingerprint: surface.clone(),
        };
        let embedding = if let Some(embedding) = cache.entries.get(&key) {
            embedding.clone()
        } else {
            let embedding = unit_norm(encoder.encode_context(&text, strategy)?);
            cache.entries.insert(key.clone(), embedding.clone());
            embedding
        };
        let score = query_embedding
            .iter()
            .zip(embedding.iter())
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum();
        scores.push((name, score));
    }
    Ok(sorted_rank(scores))
}

fn encoder_cost_report(
    encoder: &CandleBertSequenceEncoder,
    dev: &[ToolAdvisorCase],
    model_load_ms: f64,
) -> Result<EncoderCostReport> {
    let case = expand_universe(dev, 64)?
        .into_iter()
        .next()
        .context("M002 64-tool probe universe is empty")?;
    let (query, descriptors) = semantic_texts(&case, "signal-v2-flat")?;
    let descriptor = descriptors
        .first()
        .context("M002 probe case has no deferred descriptors")?
        .2
        .clone();
    let strategy = PoolingStrategy::Mean;
    let started = Instant::now();
    encoder.encode_context(&query, strategy)?;
    let first_query_encode_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    encoder.encode_context(&descriptor, strategy)?;
    let first_descriptor_encode_ms = started.elapsed().as_secs_f64() * 1000.0;
    let warm_repetitions = 5;
    let mut query_costs = Vec::with_capacity(warm_repetitions);
    let mut descriptor_costs = Vec::with_capacity(warm_repetitions);
    for _ in 0..warm_repetitions {
        let started = Instant::now();
        encoder.encode_context(&query, strategy)?;
        query_costs.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        encoder.encode_context(&descriptor, strategy)?;
        descriptor_costs.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(EncoderCostReport {
        model_load_ms,
        first_query_encode_ms,
        first_descriptor_encode_ms,
        warm_query_encode_p50_ms: percentile(&query_costs, 0.50),
        warm_descriptor_encode_p50_ms: percentile(&descriptor_costs, 0.50),
        warm_repetitions,
    })
}

pub(super) fn current_step_dev_cases() -> Result<(Vec<ToolAdvisorCase>, String, String)> {
    let corpus = load_cases(None)?;
    let dataset = dataset_fingerprint(&corpus)?;
    let partition = partition_cases(&corpus);
    let mut dev = partition
        .dev_cases
        .iter()
        .map(|index| corpus[*index].clone())
        .collect::<Vec<_>>();
    if dev.len() != M004_DEV_CASES {
        return Err(anyhow!("M002 dev split changed: {} cases", dev.len()));
    }
    let dev_fingerprint = dataset_fingerprint(&dev)?;
    for case in &mut dev {
        for (case_id, tool) in M006_EXCLUDED {
            if case.case_id == case_id {
                case.relevance.remove(tool);
                case.preferred_order.retain(|name| name != tool);
            }
        }
        case.validate().with_context(|| {
            format!(
                "validate M006 current-step projection for case {}",
                case.case_id
            )
        })?;
    }
    Ok((dev, dataset, dev_fingerprint))
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn add_group(
    groups: &mut BTreeMap<String, (usize, usize)>,
    key: String,
    relevant: usize,
    recovered: usize,
) {
    let entry = groups.entry(key).or_default();
    entry.0 += relevant;
    entry.1 += recovered;
}

fn group_report(groups: BTreeMap<String, (usize, usize)>) -> BTreeMap<String, SliceRecall> {
    groups
        .into_iter()
        .map(|(key, (relevant, recovered))| {
            (
                key,
                SliceRecall {
                    relevant,
                    recovered,
                    recall: if relevant == 0 {
                        0.0
                    } else {
                        recovered as f64 / relevant as f64
                    },
                },
            )
        })
        .collect()
}

fn frontier_point(
    arm: &str,
    universe: usize,
    k: usize,
    evaluated: &[EvaluatedCase],
) -> M002FrontierPoint {
    let mut relevant = 0usize;
    let mut recovered = 0usize;
    let mut primary_relevant = 0usize;
    let mut primary_recovered = 0usize;
    let mut secondary_relevant = 0usize;
    let mut secondary_recovered = 0usize;
    let mut per_tool = BTreeMap::<String, (usize, usize)>::new();
    let mut by_identity = BTreeMap::<String, (usize, usize)>::new();
    let mut by_schema = BTreeMap::<String, (usize, usize)>::new();
    let mut violations = 0usize;
    let mut latencies = Vec::new();
    let mut cache_entries = 0usize;
    let mut cache_bytes = 0usize;
    for evaluated_case in evaluated {
        let case = &evaluated_case.case;
        let top: BTreeSet<_> = evaluated_case
            .ranked
            .iter()
            .take(k)
            .map(|(name, _)| name.as_str())
            .collect();
        let allowed: BTreeSet<_> = deferred_candidates(case)
            .into_iter()
            .map(|candidate| candidate.name.as_str())
            .collect();
        violations += evaluated_case
            .ranked
            .iter()
            .take(k)
            .filter(|(name, _)| !allowed.contains(name.as_str()))
            .count();
        let max_grade = case.relevance.values().copied().max().unwrap_or(0);
        for (tool, grade) in &case.relevance {
            let hit = top.contains(tool.as_str());
            relevant += 1;
            recovered += usize::from(hit);
            add_group(&mut per_tool, tool.clone(), 1, usize::from(hit));
            if *grade == max_grade {
                primary_relevant += 1;
                primary_recovered += usize::from(hit);
            } else {
                secondary_relevant += 1;
                secondary_recovered += usize::from(hit);
            }
            if let Some(candidate) = case.candidates.iter().find(|c| c.name == *tool) {
                add_group(
                    &mut by_identity,
                    if candidate.synthetic_identity {
                        "synthetic_or_external".into()
                    } else {
                        "native".into()
                    },
                    1,
                    usize::from(hit),
                );
                add_group(
                    &mut by_schema,
                    if live_builtin_schema(&candidate.name).is_some() {
                        "schema_present".into()
                    } else {
                        "schema_absent".into()
                    },
                    1,
                    usize::from(hit),
                );
            }
        }
        latencies.push(evaluated_case.latency_ms);
        cache_entries = cache_entries.max(evaluated_case.cache_entries);
        cache_bytes = cache_bytes.max(evaluated_case.cache_bytes);
    }
    M002FrontierPoint {
        arm: arm.to_string(),
        universe,
        k,
        cases: evaluated.len(),
        eligible_relevant: relevant,
        recovered_relevant: recovered,
        recall: if relevant == 0 {
            0.0
        } else {
            recovered as f64 / relevant as f64
        },
        primary_relevant,
        primary_recovered,
        primary_recall: if primary_relevant == 0 {
            0.0
        } else {
            primary_recovered as f64 / primary_relevant as f64
        },
        secondary_relevant,
        secondary_recovered,
        secondary_recall: if secondary_relevant == 0 {
            0.0
        } else {
            secondary_recovered as f64 / secondary_relevant as f64
        },
        per_tool: group_report(per_tool),
        builtin_vs_synthetic: group_report(by_identity),
        schema_presence: group_report(by_schema),
        latency_p50_ms: percentile(&latencies, 0.50),
        latency_p95_ms: percentile(&latencies, 0.95),
        latency_max_ms: latencies.iter().copied().fold(0.0, f64::max),
        descriptor_cache_entries: cache_entries,
        descriptor_cache_bytes: cache_bytes,
        authority_violations: violations,
    }
}

fn m006_required_hits(universe: usize) -> usize {
    let threshold = match universe {
        64 => GATE_RECALL_64,
        128 => GATE_RECALL_128,
        _ => GATE_RECALL_256,
    };
    (69.0 * threshold).ceil() as usize
}

fn choose_operating_point(frontier: &[M002FrontierPoint]) -> (Option<String>, Option<usize>) {
    let mut candidates = Vec::new();
    let arms: BTreeSet<_> = frontier.iter().map(|point| point.arm.clone()).collect();
    for arm in arms {
        for k in PREREG_PRIMARY_KS {
            let find = |universe| {
                frontier
                    .iter()
                    .find(|point| point.arm == arm && point.universe == universe && point.k == k)
            };
            let (Some(p64), Some(p128), Some(p256)) = (find(64), find(128), find(256)) else {
                continue;
            };
            if p64.recovered_relevant >= m006_required_hits(64)
                && p128.recovered_relevant >= m006_required_hits(128)
                && p256.recovered_relevant >= m006_required_hits(256)
                && p64.authority_violations + p128.authority_violations + p256.authority_violations
                    == 0
            {
                candidates.push((
                    k,
                    p256.latency_p95_ms,
                    p256.descriptor_cache_bytes,
                    arm.clone(),
                ));
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
    });
    candidates
        .into_iter()
        .next()
        .map(|(k, _, _, arm)| (Some(arm), Some(k)))
        .unwrap_or((None, None))
}

fn m007_fingerprint() -> Result<String> {
    let receipt: serde_json::Value = serde_json::from_str(include_str!(
        "../../assets/tool-advisor/retrieval-signal-m002-preregistration.json"
    ))?;
    let bytes = serde_json::to_vec(&receipt["spec"])?;
    let spec = m007_preregistration_spec();
    if receipt["spec"] != spec {
        return Err(anyhow!(
            "committed M007 receipt differs from typed scoring contract"
        ));
    }
    let canonical = canonical_json_bytes(&spec)?;
    let hash = hex::encode(Sha256::digest(canonical));
    if receipt["sha256"].as_str() != Some(hash.as_str()) {
        return Err(anyhow!(
            "committed M007 receipt fingerprint does not match its spec"
        ));
    }
    let _ = bytes;
    Ok(hash)
}

fn canonical_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    match value {
        serde_json::Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let mut bytes = vec![b'{'];
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend(serde_json::to_vec(key)?);
                bytes.push(b':');
                bytes.extend(canonical_json_bytes(&object[*key])?);
            }
            bytes.push(b'}');
            Ok(bytes)
        }
        serde_json::Value::Array(values) => {
            let mut bytes = vec![b'['];
            for (index, item) in values.iter().enumerate() {
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend(canonical_json_bytes(item)?);
            }
            bytes.push(b']');
            Ok(bytes)
        }
        _ => Ok(serde_json::to_vec(value)?),
    }
}

fn digest_file(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(std::fs::read(path)?)))
}

fn universe_fingerprint(cases: &[ToolAdvisorCase], sizes: &[usize]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"retrieval-signal-m002-universe-v1\n");
    hasher.update(fingerprint_cases_without_case_size_validation(cases)?.as_bytes());
    for size in sizes {
        let expanded = expand_universe(cases, *size)?;
        hasher.update(size.to_le_bytes());
        hasher.update(fingerprint_cases_without_case_size_validation(&expanded)?.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

fn fingerprint_cases_without_case_size_validation(cases: &[ToolAdvisorCase]) -> Result<String> {
    let mut ordered = cases.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    let mut hasher = Sha256::new();
    for case in ordered {
        // Expanded 256-candidate universes intentionally exceed the generic
        // persisted-case limit; expand_universe has already checked their
        // dedicated candidate-count and authority contract.
        hasher.update(serde_json::to_vec(case)?);
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn run_sweep() -> Result<M002SweepReceipt> {
    let (dev, dataset_hash, dev_hash) = current_step_dev_cases()?;
    let eligible: usize = dev.iter().map(|case| case.relevance.len()).sum();
    if eligible != 69 {
        return Err(anyhow!(
            "M006-adjusted dev relevant count is {eligible}; expected 69"
        ));
    }
    let universe_fingerprint = universe_fingerprint(&dev, &PREREG_UNIVERSES)?;
    let manifest_path = PathBuf::from(M002_ENCODER_MANIFEST);
    let manifest_bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("read pinned encoder manifest {}", manifest_path.display()))?;
    let manifest_hash = hex::encode(Sha256::digest(&manifest_bytes));
    let model_load_start = Instant::now();
    let encoder = CandleBertSequenceEncoder::load(&manifest_path, &Device::Cpu)
        .context("load M002 pinned MiniLM encoder")?;
    let model_load_ms = model_load_start.elapsed().as_secs_f64() * 1000.0;
    if !encoder.assets.manifest.hashes.contains_key("weights")
        || !encoder.assets.manifest.hashes.contains_key("vocabulary")
    {
        return Err(anyhow!(
            "verified encoder manifest is missing weight/vocabulary hashes"
        ));
    }
    let encoder_version = format!(
        "{}:{}",
        manifest_hash, encoder.assets.manifest.hashes["vocabulary"]
    );
    let encoder_costs = encoder_cost_report(&encoder, &dev, model_load_ms)?;
    let mut frontier = Vec::new();
    let mut evaluated_by_arm_universe = BTreeMap::<(String, usize), Vec<EvaluatedCase>>::new();
    let mut universe_counts = BTreeMap::new();
    for size in PREREG_UNIVERSES {
        let mut semantic_caches = BTreeMap::<String, DescriptorCache>::new();
        let expanded = expand_universe(&dev, size)?;
        let count: usize = expanded.iter().map(|case| case.relevance.len()).sum();
        universe_counts.insert(size, count);
        if count != 69 {
            return Err(anyhow!(
                "expanded M006 universe {size} contains {count} relevant labels"
            ));
        }
        for case in expanded {
            for arm in DETERMINISTIC_LEXICAL_VARIANTS {
                let start = Instant::now();
                let ranked = lexical_rank(&case, arm)?;
                let evaluated = EvaluatedCase {
                    case: case.clone(),
                    ranked,
                    latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                    cache_entries: 0,
                    cache_bytes: 0,
                };
                evaluated_by_arm_universe
                    .entry((arm.to_string(), size))
                    .or_default()
                    .push(evaluated);
            }
            for arm in DETERMINISTIC_SEMANTIC_VARIANTS {
                for pool in DETERMINISTIC_POOLING_VARIANTS {
                    let name = format!("{arm}/{pool}");
                    let start = Instant::now();
                    let cache = semantic_caches.entry(name.clone()).or_default();
                    let ranked =
                        semantic_rank(&case, arm, pool, &encoder, &encoder_version, cache)?;
                    let evaluated = EvaluatedCase {
                        case: case.clone(),
                        ranked,
                        latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                        cache_entries: cache.entries.len(),
                        cache_bytes: cache.bytes(),
                    };
                    evaluated_by_arm_universe
                        .entry((name, size))
                        .or_default()
                        .push(evaluated);
                }
            }
        }
    }
    for ((arm, universe), evaluated) in &evaluated_by_arm_universe {
        for k in PREREG_PRIMARY_KS {
            frontier.push(frontier_point(arm, *universe, k, evaluated));
        }
    }
    frontier.sort_by(|left, right| {
        left.arm
            .cmp(&right.arm)
            .then_with(|| left.universe.cmp(&right.universe))
            .then_with(|| left.k.cmp(&right.k))
    });
    let authority_violations = frontier
        .iter()
        .map(|point| point.authority_violations)
        .sum();
    let (selected_arm, selected_k) = choose_operating_point(&frontier);
    let bm25_256 = evaluated_by_arm_universe
        .get(&("v1-bm25-baseline".into(), 256))
        .context("baseline 256 frontier missing")?;
    let misses = super::retrieval_signal::run_miss_audit()?;
    let mut persistent_misses = Vec::new();
    for miss in misses.occurrences {
        let universe_case = expand_universe(&dev, 256)?
            .into_iter()
            .find(|case| case.case_id == miss.case_id)
            .context("persistent miss case absent from expanded dev")?;
        let prior_bm25_rank = super::retrieval_signal::bm25_full_order(&universe_case)
            .iter()
            .position(|(name, _)| name == &miss.tool)
            .map(|index| index + 1)
            .unwrap_or(usize::MAX);
        let mut rank_by_arm = BTreeMap::new();
        let mut entered_k = BTreeMap::new();
        for arm in DETERMINISTIC_LEXICAL_VARIANTS {
            if let Some(result) = evaluated_by_arm_universe
                .get(&(arm.to_string(), 256))
                .and_then(|cases| cases.iter().find(|item| item.case.case_id == miss.case_id))
            {
                let rank = result
                    .ranked
                    .iter()
                    .position(|(name, _)| name == &miss.tool)
                    .map(|i| i + 1)
                    .unwrap_or(usize::MAX);
                rank_by_arm.insert(arm.to_string(), rank);
                for k in PREREG_PRIMARY_KS {
                    entered_k.insert(format!("{arm}/K{k}"), rank <= k);
                }
            }
        }
        for arm in DETERMINISTIC_SEMANTIC_VARIANTS {
            for pool in DETERMINISTIC_POOLING_VARIANTS {
                let label = format!("{arm}/{pool}");
                if let Some(result) = evaluated_by_arm_universe
                    .get(&(label.clone(), 256))
                    .and_then(|cases| cases.iter().find(|item| item.case.case_id == miss.case_id))
                {
                    let rank = result
                        .ranked
                        .iter()
                        .position(|(name, _)| name == &miss.tool)
                        .map(|i| i + 1)
                        .unwrap_or(usize::MAX);
                    rank_by_arm.insert(label.clone(), rank);
                    for k in PREREG_PRIMARY_KS {
                        entered_k.insert(format!("{label}/K{k}"), rank <= k);
                    }
                }
            }
        }
        persistent_misses.push(PersistentMissResult {
            tool: miss.tool,
            case_id: miss.case_id,
            prior_bm25_rank,
            m002_rank_by_arm: rank_by_arm,
            entered_k,
        });
    }
    if bm25_256.is_empty() {
        return Err(anyhow!("empty baseline frontier"));
    }
    let decision_path = Path::new("assets/tool-advisor/retrieval-signal-m006-decision.json");
    let m006_decision_sha256 = digest_file(decision_path)?;
    let disposition = if selected_arm.is_some() {
        "positive: deterministic point clears all frozen gates".to_string()
    } else {
        "negative-valid: deterministic point misses at least one frozen gate; conditional M003 may proceed".to_string()
    };
    Ok(M002SweepReceipt {
        schema_version: 1,
        protocol: M002_PROTOCOL.into(),
        preregistration_sha256: m007_fingerprint()?,
        m006_decision_sha256,
        dataset_sha256: dataset_hash,
        dev_partition_sha256: dev_hash,
        universe_fingerprint,
        encoder_manifest_sha256: manifest_hash,
        encoder_costs,
        cases: dev.len(),
        eligible_relevant_by_universe: universe_counts,
        required_hits: [
            (64, m006_required_hits(64)),
            (128, m006_required_hits(128)),
            (256, m006_required_hits(256)),
        ]
        .into_iter()
        .collect(),
        frontier,
        persistent_misses,
        authority_violations,
        selected_arm,
        selected_k,
        disposition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m002_current_step_denominator_is_69_and_universe_preserves_labels() {
        let (dev, _, _) = current_step_dev_cases().expect("M006 adjusted dev split");
        assert_eq!(
            dev.iter().map(|case| case.relevance.len()).sum::<usize>(),
            69
        );
        for size in PREREG_UNIVERSES {
            let expanded = expand_universe(&dev, size).expect("expand dev universe");
            assert_eq!(
                expanded
                    .iter()
                    .map(|case| case.relevance.len())
                    .sum::<usize>(),
                69
            );
            assert!(expanded
                .iter()
                .all(|case| deferred_candidates(case).len() == size));
        }
    }

    #[test]
    fn m002_current_step_projection_removes_excluded_labels_consistently_without_mutating_corpus() {
        let corpus = load_cases(None).expect("frozen corpus");
        let source_fingerprint = dataset_fingerprint(&corpus).expect("source fingerprint");
        let partition = partition_cases(&corpus);
        let source_dev: BTreeMap<_, _> = partition
            .dev_cases
            .iter()
            .map(|index| (corpus[*index].case_id.as_str(), &corpus[*index]))
            .collect();

        let (projected, projected_source_fingerprint, _) =
            current_step_dev_cases().expect("current-step dev view");
        assert_eq!(projected_source_fingerprint, source_fingerprint);
        assert_eq!(
            fingerprint_cases_without_case_size_validation(&projected)
                .expect("current-step fingerprint"),
            dataset_fingerprint(&projected).expect("validated current-step fingerprint")
        );
        assert_eq!(
            dataset_fingerprint(&corpus).expect("corpus remains unchanged"),
            source_fingerprint
        );
        assert_eq!(
            projected
                .iter()
                .filter(|case| M006_EXCLUDED
                    .iter()
                    .any(|(case_id, _)| *case_id == case.case_id))
                .count(),
            M006_EXCLUDED.len()
        );
        assert_eq!(
            projected
                .iter()
                .map(|case| case.relevance.len())
                .sum::<usize>(),
            69
        );
        let universe_hash = universe_fingerprint(&projected, &PREREG_UNIVERSES)
            .expect("all universe fingerprints are computed before encoder loading");
        assert_eq!(
            universe_hash,
            universe_fingerprint(&projected, &PREREG_UNIVERSES)
                .expect("universe fingerprint is deterministic")
        );

        for case in &projected {
            let source = source_dev
                .get(case.case_id.as_str())
                .expect("projected case belongs to frozen dev split");
            let excluded = M006_EXCLUDED
                .iter()
                .find(|(case_id, _)| *case_id == case.case_id);
            let mut expected_relevance = source.relevance.clone();
            let mut expected_preferred_order = source.preferred_order.clone();
            if let Some((_, tool)) = excluded {
                assert!(expected_relevance.remove(*tool).is_some());
                expected_preferred_order.retain(|name| name != tool);
                assert!(!case.relevance.contains_key(*tool));
                assert!(!case.preferred_order.iter().any(|name| name == tool));
            }
            assert_eq!(case.relevance, expected_relevance, "{}", case.case_id);
            assert_eq!(
                case.preferred_order, expected_preferred_order,
                "{}",
                case.case_id
            );
            case.validate().expect("projected case is coherent");
        }

        for size in PREREG_UNIVERSES {
            let expanded = expand_universe(&projected, size).expect("projected universe");
            if size <= 128 {
                assert_eq!(
                    fingerprint_cases_without_case_size_validation(&expanded)
                        .expect("expanded fingerprint"),
                    dataset_fingerprint(&expanded).expect("validated expanded fingerprint")
                );
            }
            assert_eq!(
                expanded
                    .iter()
                    .map(|case| case.relevance.len())
                    .sum::<usize>(),
                69
            );
            if size <= 128 {
                for case in &expanded {
                    case.validate()
                        .unwrap_or_else(|error| panic!("{}: {error:#}", case.case_id));
                }
            }
        }
    }

    #[test]
    fn m002_lexical_arms_rank_only_deferred_candidates_deterministically() {
        let (dev, _, _) = current_step_dev_cases().expect("dev");
        let expanded = expand_universe(&dev, 64).expect("universe");
        let case = expanded
            .iter()
            .find(|case| !case.relevance.is_empty())
            .unwrap();
        for arm in DETERMINISTIC_LEXICAL_VARIANTS {
            let first = lexical_rank(case, arm).expect("rank");
            let second = lexical_rank(case, arm).expect("repeat rank");
            assert_eq!(first, second);
            assert_eq!(first.len(), 64);
            assert!(first.iter().all(|(name, _)| {
                case.candidates
                    .iter()
                    .any(|candidate| candidate.name == *name && candidate.disclosure == "deferred")
            }));
        }
    }

    #[test]
    fn m002_field_weighted_bm25_formula_is_finite_and_order_stable() {
        let docs = vec![
            (
                "alpha".into(),
                vec![
                    vec!["open".into(), "file".into()],
                    vec![],
                    vec![],
                    vec![],
                    vec!["path".into(), "required".into()],
                    vec![],
                ],
            ),
            (
                "beta".into(),
                vec![
                    vec!["read".into()],
                    vec![],
                    vec![],
                    vec![],
                    vec!["path".into()],
                    vec![],
                ],
            ),
        ];
        let query = vec![
            vec!["open".into(), "file".into()],
            vec![],
            vec![],
            vec![],
            vec![],
        ];
        let ranked = sorted_rank(field_weighted_bm25(&query, &docs));
        assert_eq!(ranked[0].0, "alpha");
        assert!(ranked.iter().all(|(_, score)| score.is_finite()));
    }

    #[test]
    fn semantic_cache_identity_distinguishes_descriptor_arms() {
        let base = RetrievalCacheKeyV2 {
            representation_schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
            semantic_arm: "signal-v2-flat".into(),
            descriptor_fingerprint: "same-descriptor".into(),
            encoder_tokenizer_version: "pinned-model".into(),
            pooling: "mean".into(),
            surface_fingerprint: "same-surface".into(),
        };
        let mut labelled = base.clone();
        labelled.semantic_arm = "signal-v2-field-labelled-descriptor".into();
        assert_ne!(base, labelled);
        let mut cache = DescriptorCache::default();
        cache.entries.insert(base, vec![1.0]);
        assert!(cache.entries.get(&labelled).is_none());
    }

    #[test]
    fn m002_frontier_recall_slices_use_current_step_labels() {
        let (dev, _, _) = current_step_dev_cases().expect("dev");
        let cases = expand_universe(&dev, 64).expect("universe");
        let evaluated = cases
            .iter()
            .map(|case| EvaluatedCase {
                case: case.clone(),
                ranked: case
                    .relevance
                    .keys()
                    .map(|name| (name.clone(), 1.0))
                    .collect(),
                latency_ms: 1.0,
                cache_entries: 0,
                cache_bytes: 0,
            })
            .collect::<Vec<_>>();
        let point = frontier_point("test", 64, 16, &evaluated);
        assert_eq!(point.eligible_relevant, 69);
        assert_eq!(point.recovered_relevant, 69);
        assert_eq!(point.authority_violations, 0);
    }

    #[test]
    #[ignore = "full pinned-MiniLM M002 sweep; invoke explicitly after all preregistration gates pass"]
    fn m002_preregistered_dev_frontier_sweep() {
        let receipt = run_sweep().expect("M002 sweep");
        let bytes = serde_json::to_vec_pretty(&receipt).expect("serialize receipt");
        std::fs::create_dir_all(M002_OUTPUT_DIR).expect("create output directory");
        std::fs::write(Path::new(M002_OUTPUT_DIR).join("frontier.json"), &bytes)
            .expect("write full report");
        std::fs::write(M002_RECEIPT_PATH, bytes).expect("write committed receipt");
        println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
    }
}
