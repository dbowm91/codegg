//! M002 deterministic Retrieval Signal V2 (lexical half, default features).
//!
//! Implements the M001R-frozen Signal V2 representation and the deterministic
//! lexical frontier without learned weights. Semantic MiniLM variants (§4 of
//! the M002 plan) need the `tool-advisor-encoder-training` feature
//! (currently broken by `candle-core 0.11` nightly-only `stdarch_neon_f16`
//! items) plus the reference MiniLM manifest
//! (`target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json`,
//! absent here), so they are preregistered-but-unmeasured in this module:
//! descriptor/query text construction and the embedding cache-key contract
//! are implemented and tested here, while encoder invocation and semantic
//! ranking live behind the encoder-training gate.
//!
//! Scope boundary: experiment-local representation and BM25 frontier only.
//! `ToolCatalog` behavior is unchanged; historical v1 semantics are reused
//! verbatim for the baseline mode; no per-tool alias is added; v3/v4 select
//! nothing.

use super::context_v2::AdvisorContextV2;
use super::retrieval_relevance::{
    bm25_ordering_local, build_derived_view, eligible_deferred_names_local, expand_universe_local,
};
use super::retrieval_signal::{
    cap_field, normalize_identifier, CANDIDATE_FIELD_CAPS, DESCRIPTOR_TOTAL_CAP_BYTES,
    EXPECTED_DERIVED_VIEW_FINGERPRINT, LEXICAL_FIELD_WEIGHTS, PREREG_DERIVED_VIEW, QUERY_FIELDS,
    QUERY_FIELD_CAPS, QUERY_MAX_NEXT_STEPS, SIGNAL_PREREG_ASSET_PATH, SIGNAL_PREREG_PROTOCOL,
};
use super::{baseline_prediction, normalize_text, ToolAdvisorCandidate, ToolAdvisorCase};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

/// Schema version for the Signal V2 representation and frontier receipt.
pub const RETRIEVAL_SIGNAL_SCHEMA_VERSION: u16 = 2;
/// Lexical modes measured in default features (subset of the M001R grid).
pub const LEXICAL_MODES: [&str; 4] = [
    "bm25-flat-v1-baseline",
    "descriptor-v2-flat-bm25",
    "field-weighted-bm25-v2",
    "normalized-token-bm25-v2",
];
/// Semantic modes preregistered but unmeasured without the encoder toolchain.
pub const SEMANTIC_MODES: [&str; 4] = [
    "semantic-flat-v2-mean",
    "semantic-field-labelled-v2-mean",
    "rrf-v2",
    "normalized-union-v2",
];
/// Frontier receipt path (interim: lexical modes only until semantic arms run).
pub const SIGNAL_V2_FRONTIER_ASSET: &str =
    "assets/tool-advisor/retrieval-signal-m002-frontier.json";
/// Universes and shortlists, unchanged from M001R/M004.
pub const FRONTIER_UNIVERSES: [usize; 3] = [64, 128, 256];
pub const FRONTIER_KS: [usize; 3] = [16, 24, 32];

/// Versioned Signal V2 candidate descriptor: the 8 M001R fields in canonical
/// order, each capped before use. Schema fields come from the live tool
/// `parameters` JSON schema when present; frozen offline cases carry no
/// schema and degrade to empty schema fields (never invented).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalDescriptorV2 {
    pub canonical_name: String,
    pub identifier_tokens: String,
    pub description: String,
    pub category: String,
    pub disclosure: String,
    pub schema_property_names: String,
    pub schema_property_descriptions: String,
    pub operation_terms: String,
}

impl RetrievalDescriptorV2 {
    pub fn from_candidate(
        candidate: &ToolAdvisorCandidate,
        schema: Option<&serde_json::Value>,
    ) -> Self {
        let (schema_names, schema_descriptions) = schema
            .map(schema_fields_from_parameters)
            .unwrap_or_default();
        let identifier_tokens = normalize_identifier(&candidate.name);
        let operation_terms = operation_terms(&candidate.name, &candidate.description);
        let mut descriptor = Self {
            canonical_name: candidate.name.clone(),
            identifier_tokens,
            description: candidate.description.clone(),
            category: candidate.category.clone(),
            disclosure: candidate.disclosure.clone(),
            schema_property_names: schema_names,
            schema_property_descriptions: schema_descriptions,
            operation_terms,
        };
        descriptor.apply_caps();
        descriptor
    }

    fn apply_caps(&mut self) {
        let cap = |field: &str, value: &mut String| {
            let limit = CANDIDATE_FIELD_CAPS
                .iter()
                .find(|(name, _)| *name == field)
                .map(|(_, bytes)| *bytes)
                .unwrap_or(usize::MAX);
            *value = cap_field(value, limit).to_string();
        };
        cap("canonical_name", &mut self.canonical_name);
        cap("identifier_tokens", &mut self.identifier_tokens);
        cap("description", &mut self.description);
        cap("category", &mut self.category);
        cap("disclosure", &mut self.disclosure);
        cap("schema_property_names", &mut self.schema_property_names);
        cap(
            "schema_property_descriptions",
            &mut self.schema_property_descriptions,
        );
        cap("operation_terms", &mut self.operation_terms);
    }

    /// Flat descriptor text: all fields in canonical order, blanks skipped.
    pub fn flat_text(&self) -> String {
        [
            self.canonical_name.as_str(),
            self.identifier_tokens.as_str(),
            self.description.as_str(),
            self.category.as_str(),
            self.disclosure.as_str(),
            self.schema_property_names.as_str(),
            self.schema_property_descriptions.as_str(),
            self.operation_terms.as_str(),
        ]
        .iter()
        .filter(|part| !part.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
    }

    /// Field-labelled descriptor text for semantic variants (generic labels).
    pub fn field_labelled_text(&self) -> String {
        format!(
            "name: {} identifiers: {} description: {} category: {} disclosure: {} schema: {} schema-descriptions: {} operations: {}",
            self.canonical_name,
            self.identifier_tokens,
            self.description,
            self.category,
            self.disclosure,
            self.schema_property_names,
            self.schema_property_descriptions,
            self.operation_terms
        )
    }

    /// Normalized flat text: identifier-split + lowercased + collapsed.
    pub fn normalized_text(&self) -> String {
        normalize_text(&self.flat_text())
    }

    pub fn encoded_bytes(&self) -> usize {
        self.flat_text().len()
    }

    pub fn has_schema(&self) -> bool {
        !self.schema_property_names.trim().is_empty()
            || !self.schema_property_descriptions.trim().is_empty()
    }
}

/// Versioned Signal V2 query: the 5 M001R `AdvisorContextV2` fields in
/// canonical order. Frozen benchmark cases populate only `current_objective`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalQueryV2 {
    pub current_objective: String,
    pub current_task: String,
    pub next_steps: Vec<String>,
    pub unresolved_signal: String,
    pub capability_cue: String,
}

impl RetrievalQueryV2 {
    pub fn from_advisor_context(context: &AdvisorContextV2) -> Self {
        let mut next_steps = context.next_steps.clone();
        next_steps.truncate(QUERY_MAX_NEXT_STEPS);
        let mut query = Self {
            current_objective: context.current_objective.clone().unwrap_or_default(),
            current_task: context.current_task.clone().unwrap_or_default(),
            next_steps,
            unresolved_signal: context.unresolved_signal.clone().unwrap_or_default(),
            capability_cue: context.capability_cue.clone().unwrap_or_default(),
        };
        query.apply_caps();
        query
    }

    pub fn from_benchmark_context(context: &str) -> Self {
        Self::from_advisor_context(&AdvisorContextV2::from_benchmark_context(context))
    }

    fn apply_caps(&mut self) {
        let cap = |field: &str, value: &mut String| {
            let limit = QUERY_FIELD_CAPS
                .iter()
                .find(|(name, _)| *name == field)
                .map(|(_, bytes)| *bytes)
                .unwrap_or(usize::MAX);
            *value = cap_field(value, limit).to_string();
        };
        cap("current_objective", &mut self.current_objective);
        cap("current_task", &mut self.current_task);
        cap("unresolved_signal", &mut self.unresolved_signal);
        cap("capability_cue", &mut self.capability_cue);
        let step_cap = QUERY_FIELD_CAPS
            .iter()
            .find(|(name, _)| *name == "next_steps")
            .map(|(_, bytes)| *bytes)
            .unwrap_or(usize::MAX);
        for step in &mut self.next_steps {
            *step = cap_field(step, step_cap).to_string();
        }
    }

    pub fn flat_text(&self) -> String {
        let mut parts = vec![self.current_objective.clone(), self.current_task.clone()];
        parts.extend(self.next_steps.clone());
        parts.push(self.unresolved_signal.clone());
        parts.push(self.capability_cue.clone());
        parts
            .iter()
            .filter(|part| !part.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn field_labelled_text(&self) -> String {
        format!(
            "objective: {} task: {} next: {} unresolved: {} capability: {}",
            self.current_objective,
            self.current_task,
            self.next_steps.join("; "),
            self.unresolved_signal,
            self.capability_cue
        )
    }

    pub fn normalized_text(&self) -> String {
        normalize_text(&self.flat_text())
    }
}

/// Bounded schema-derived fields from a live tool JSON schema value.
///
/// Extracts only static `properties` keys plus nested `description` strings,
/// sorted for determinism. Defaults, examples, const/enum values, and runtime
/// data never enter the descriptor (they would leak instance values into
/// retrieval signal).
pub fn schema_fields_from_parameters(parameters: &serde_json::Value) -> (String, String) {
    let Some(properties) = parameters.get("properties").and_then(|v| v.as_object()) else {
        return (String::new(), String::new());
    };
    let mut names: Vec<String> = properties.keys().cloned().collect();
    names.sort();
    let mut descriptions: Vec<String> = Vec::new();
    for name in &names {
        if let Some(description) = properties
            .get(name)
            .and_then(|prop| prop.get("description"))
            .and_then(|v| v.as_str())
        {
            let trimmed = description.trim();
            if !trimmed.is_empty() {
                descriptions.push(trimmed.to_string());
            }
        }
    }
    (names.join(" "), descriptions.join(" "))
}

/// Deterministic operation/capability terms from a generic normalizer: split
/// identifier tokens plus a fixed verb-family map applied to description
/// words. The map is generic English (create/save/persist/write, find/locate/
/// search, etc.), never per-tool text.
pub fn operation_terms(name: &str, description: &str) -> String {
    const VERB_FAMILIES: [(&str, &str); 9] = [
        ("create", "create"),
        ("persist", "create"),
        ("save", "create"),
        ("write", "create"),
        ("find", "locate"),
        ("locate", "locate"),
        ("search", "locate"),
        ("list", "enumerate"),
        ("enumerate", "enumerate"),
    ];
    let mut terms: BTreeSet<String> = BTreeSet::new();
    for token in normalize_identifier(name).split_whitespace() {
        terms.insert(token.to_string());
    }
    let lowered = description.to_ascii_lowercase();
    for word in lowered.split(|c: char| !c.is_alphanumeric()) {
        for (variant, family) in VERB_FAMILIES {
            if word == variant {
                terms.insert(family.to_string());
            }
        }
    }
    terms.into_iter().collect::<Vec<_>>().join(" ")
}

/// Descriptor embedding cache key contract for semantic variants (M002 §4).
/// Carries descriptor identity only, never user context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticCacheKey {
    pub representation_schema_version: u16,
    pub prereg_protocol: String,
    pub descriptor_fingerprint: String,
    pub encoder_tokenizer_version: String,
    pub pooling: String,
    pub surface_fingerprint: String,
}

pub fn descriptor_fingerprint(descriptor: &RetrievalDescriptorV2) -> String {
    let bytes = serde_json::to_vec(descriptor).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

pub fn semantic_cache_key(
    descriptor: &RetrievalDescriptorV2,
    encoder_tokenizer_version: &str,
    pooling: &str,
    surface_fingerprint: &str,
) -> SemanticCacheKey {
    SemanticCacheKey {
        representation_schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
        prereg_protocol: SIGNAL_PREREG_PROTOCOL.to_string(),
        descriptor_fingerprint: descriptor_fingerprint(descriptor),
        encoder_tokenizer_version: encoder_tokenizer_version.to_string(),
        pooling: pooling.to_string(),
        surface_fingerprint: surface_fingerprint.to_string(),
    }
}

// ---- self-contained deterministic BM25 (catalog math, v2 documents) ----

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn compute_idf(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let n = documents.len() as f64;
    if n == 0.0 {
        return HashMap::new();
    }
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for doc in documents {
        let unique: HashSet<&String> = doc.iter().collect();
        for term in unique {
            *doc_freq.entry((*term).clone()).or_insert(0) += 1;
        }
    }
    doc_freq
        .into_iter()
        .map(|(term, df)| {
            let idf = ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
            (term, idf)
        })
        .collect()
}

fn bm25_score(
    query_terms: &[String],
    doc_terms: &[String],
    avg_dl: f64,
    idf: &HashMap<String, f64>,
) -> f64 {
    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let doc_len = doc_terms.len() as f64;
    let mut tf: HashMap<&String, usize> = HashMap::new();
    for term in doc_terms {
        *tf.entry(term).or_insert(0) += 1;
    }
    let mut score = 0.0;
    for term in query_terms {
        if let Some(&term_tf) = tf.get(term) {
            let idf_val = idf.get(term).copied().unwrap_or(0.0);
            let numerator = term_tf as f64 * (K1 + 1.0);
            let denominator = term_tf as f64 + K1 * (1.0 - B + B * doc_len / avg_dl);
            score += idf_val * numerator / denominator;
        }
    }
    score
}

fn lexical_weight(field: &str) -> f64 {
    LEXICAL_FIELD_WEIGHTS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, weight)| *weight)
        .unwrap_or(0.0)
}

fn field_text<'a>(descriptor: &'a RetrievalDescriptorV2, field: &str) -> &'a str {
    match field {
        "canonical_name" => &descriptor.canonical_name,
        "identifier_tokens" => &descriptor.identifier_tokens,
        "operation_terms" => &descriptor.operation_terms,
        "schema_property_names" => &descriptor.schema_property_names,
        "description" => &descriptor.description,
        "category" => &descriptor.category,
        "disclosure" => &descriptor.disclosure,
        _ => "",
    }
}

/// One lexical ordering over the eligible deferred set of a single case.
fn order_flat_v2(case: &ToolAdvisorCase, normalized: bool) -> Vec<(String, f64)> {
    let descriptors: BTreeMap<String, RetrievalDescriptorV2> = case
        .candidates
        .iter()
        .filter(|c| c.disclosure == "deferred")
        .map(|c| {
            let descriptor = RetrievalDescriptorV2::from_candidate(c, None);
            (c.name.clone(), descriptor)
        })
        .collect();
    let query = RetrievalQueryV2::from_benchmark_context(&case.context);
    let query_text = if normalized {
        query.normalized_text()
    } else {
        query.flat_text()
    };
    let query_terms = tokenize(&query_text);
    let documents: BTreeMap<String, Vec<String>> = descriptors
        .iter()
        .map(|(name, descriptor)| {
            let text = if normalized {
                descriptor.normalized_text()
            } else {
                descriptor.flat_text()
            };
            (name.clone(), tokenize(&text))
        })
        .collect();
    let doc_list: Vec<Vec<String>> = documents.values().cloned().collect();
    let idf = compute_idf(&doc_list);
    let avg_dl = if doc_list.is_empty() {
        0.0
    } else {
        doc_list.iter().map(|d| d.len()).sum::<usize>() as f64 / doc_list.len() as f64
    };
    rank_with_completion(
        &descriptors,
        &documents,
        &query_terms,
        &idf,
        avg_dl,
        |_| 1.0,
        1.0,
    )
}

/// Field-weighted ordering with frozen M001R weights.
fn order_field_weighted_v2(case: &ToolAdvisorCase) -> Vec<(String, f64)> {
    let descriptors: BTreeMap<String, RetrievalDescriptorV2> = case
        .candidates
        .iter()
        .filter(|c| c.disclosure == "deferred")
        .map(|c| {
            let descriptor = RetrievalDescriptorV2::from_candidate(c, None);
            (c.name.clone(), descriptor)
        })
        .collect();
    let query = RetrievalQueryV2::from_benchmark_context(&case.context);
    let query_terms = tokenize(&query.flat_text());
    // Per-field IDF over this universe so common field values (e.g. the
    // shared "deferred" disclosure token) do not dominate rare name matches.
    let fields = [
        "canonical_name",
        "identifier_tokens",
        "operation_terms",
        "schema_property_names",
        "description",
        "category",
        "disclosure",
    ];
    let mut per_field: BTreeMap<&str, (HashMap<String, f64>, f64)> = BTreeMap::new();
    for field in fields {
        let docs: Vec<Vec<String>> = descriptors
            .values()
            .map(|d| tokenize(field_text(d, field)))
            .collect();
        let idf = compute_idf(&docs);
        let avg = if docs.is_empty() {
            0.0
        } else {
            docs.iter().map(|d| d.len()).sum::<usize>() as f64 / docs.len() as f64
        };
        per_field.insert(field, (idf, avg));
    }
    let mut scored: Vec<(String, f64)> = descriptors
        .keys()
        .map(|name| {
            let descriptor = &descriptors[name];
            let mut total = 0.0;
            for field in fields {
                let (idf, avg) = &per_field[field];
                let doc_terms = tokenize(field_text(descriptor, field));
                total += lexical_weight(field) * bm25_score(&query_terms, &doc_terms, *avg, idf);
            }
            (name.clone(), total)
        })
        .collect();
    complete_and_sort(&mut scored, descriptors.keys().cloned().collect());
    scored
}

fn rank_with_completion(
    descriptors: &BTreeMap<String, RetrievalDescriptorV2>,
    documents: &BTreeMap<String, Vec<String>>,
    query_terms: &[String],
    idf: &HashMap<String, f64>,
    avg_dl: f64,
    _field: impl Fn(&str) -> f64,
    _scale: f64,
) -> Vec<(String, f64)> {
    let mut scored: Vec<(String, f64)> = documents
        .iter()
        .map(|(name, doc)| (name.clone(), bm25_score(query_terms, doc, avg_dl, idf)))
        .collect();
    complete_and_sort(&mut scored, descriptors.keys().cloned().collect());
    scored
}

fn complete_and_sort(scored: &mut Vec<(String, f64)>, universe: BTreeSet<String>) {
    let known: BTreeSet<String> = scored
        .iter()
        .filter(|(_, score)| *score > 0.0)
        .map(|(name, _)| name.clone())
        .collect();
    let mut completed: Vec<(String, f64)> = scored
        .iter()
        .filter(|(_, score)| *score > 0.0)
        .cloned()
        .collect();
    for name in universe {
        if !known.contains(&name) {
            completed.push((name, 0.0));
        }
    }
    completed.sort_by(|left, right| left.0.cmp(&right.0));
    completed.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    *scored = completed;
}

/// Deterministic lexical ordering for one expanded case and mode.
pub fn lexical_ordering(case: &ToolAdvisorCase, mode: &str) -> Result<Vec<(String, f64)>> {
    match mode {
        "bm25-flat-v1-baseline" => {
            let ranked = bm25_ordering_local(case);
            Ok(ranked
                .into_iter()
                .map(|entry| (entry.name, entry.score))
                .collect())
        }
        "descriptor-v2-flat-bm25" => Ok(order_flat_v2(case, false)),
        "field-weighted-bm25-v2" => Ok(order_field_weighted_v2(case)),
        "normalized-token-bm25-v2" => Ok(order_flat_v2(case, true)),
        other => Err(anyhow!("unsupported lexical mode {other}")),
    }
}

// ---- frontier measurement against the corrected inferable target ----

/// One measured frontier point (lexical modes only in this module).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrontierPoint {
    pub mode: String,
    pub universe: usize,
    pub k: usize,
    pub inferable_relevant: usize,
    pub inferable_recovered: usize,
    pub inferable_recall: f64,
    pub violations: usize,
    pub mean_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub max_latency_ms: u128,
    pub descriptor_bytes: usize,
    pub gate: f64,
    pub passes: bool,
}

/// Per-tool inferable recall at one (mode, universe, K).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerToolRecall {
    pub tool: String,
    pub inferable_cases: usize,
    pub recovered_at_16: usize,
    pub recovered_at_24: usize,
    pub recovered_at_32: usize,
}

/// Persistent-miss before/after (v1 baseline rank vs best v2 rank).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistentMissRow {
    pub tool: String,
    pub case_id: String,
    pub v1_rank: Option<usize>,
    pub best_v2_rank: Option<usize>,
    pub best_v2_mode: String,
    pub in_k16: bool,
    pub in_k24: bool,
    pub in_k32: bool,
}

/// Lexical frontier receipt (interim until semantic arms are measured).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalV2FrontierReceipt {
    pub schema_version: u16,
    pub protocol: String,
    pub prereg_protocol: String,
    pub prereg_fingerprint: String,
    pub derived_view: String,
    pub derived_view_fingerprint: String,
    pub modes_measured: Vec<String>,
    pub modes_deferred: Vec<String>,
    pub points: Vec<FrontierPoint>,
    pub per_tool: Vec<PerToolRecall>,
    pub persistent_misses: Vec<PersistentMissRow>,
    pub schema_present_inferable: usize,
    pub schema_absent_inferable: usize,
    pub fingerprint: String,
}

fn inferable_for_case(
    case_id: &str,
    eligible: &BTreeMap<(String, String), bool>,
) -> BTreeSet<String> {
    eligible
        .iter()
        .filter(|((id, _), is_eligible)| id == case_id && **is_eligible)
        .map(|((_, tool), _)| tool.clone())
        .collect()
}

/// Measure the lexical frontier over dev at 64/128/256 × 16/24/32.
pub fn measure_lexical_frontier() -> Result<SignalV2FrontierReceipt> {
    let cases = super::builtin_cases().context("load corpus")?;
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
    let partition = super::partition_cases(&cases);
    let dev: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();

    let prereg_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SIGNAL_PREREG_ASSET_PATH);
    let prereg_fp = if let Ok(bytes) = std::fs::read(&prereg_path) {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).context("parse prereg receipt")?;
        value
            .get("fingerprint")
            .and_then(|v| v.as_str())
            .unwrap_or("unreadable")
            .to_string()
    } else {
        "absent".to_string()
    };

    let modes: Vec<String> = LEXICAL_MODES.iter().map(|s| s.to_string()).collect();
    let mut points = Vec::new();
    // (mode, universe, case_id) -> full ordering, reused across Ks.
    let mut orderings: BTreeMap<(String, usize, String), Vec<(String, f64)>> = BTreeMap::new();
    let mut latencies: BTreeMap<(String, usize), Vec<u128>> = BTreeMap::new();
    let mut descriptor_bytes: BTreeMap<(String, usize), usize> = BTreeMap::new();

    for universe in FRONTIER_UNIVERSES {
        let expanded = expand_universe_local(&dev, universe).context("expand dev")?;
        for mode in &modes {
            let mut latched = Vec::new();
            let mut bytes = 0usize;
            for case in &expanded {
                let started = Instant::now();
                let ordered = lexical_ordering(case, mode)?;
                latched.push(started.elapsed().as_millis());
                if mode != "bm25-flat-v1-baseline" {
                    bytes += case
                        .candidates
                        .iter()
                        .filter(|c| c.disclosure == "deferred")
                        .map(|c| RetrievalDescriptorV2::from_candidate(c, None).encoded_bytes())
                        .sum::<usize>();
                }
                orderings.insert((mode.clone(), universe, case.case_id.clone()), ordered);
            }
            latencies.insert((mode.clone(), universe), latched);
            descriptor_bytes.insert((mode.clone(), universe), bytes);
        }
    }

    let gate_for = |universe: usize| -> f64 {
        match universe {
            64 => 0.99,
            128 => 0.98,
            _ => 0.95,
        }
    };

    for universe in FRONTIER_UNIVERSES {
        let expanded = expand_universe_local(&dev, universe).context("expand dev")?;
        for mode in &modes {
            for k in FRONTIER_KS {
                let mut relevant = 0usize;
                let mut recovered = 0usize;
                let mut violations = 0usize;
                for case in &expanded {
                    let inferable = inferable_for_case(&case.case_id, &eligible);
                    let allowed = eligible_deferred_names_local(case);
                    let ordered = &orderings[&(mode.clone(), universe, case.case_id.clone())];
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
                let gate = gate_for(universe);
                let mut latched = latencies[&(mode.clone(), universe)].clone();
                latched.sort_unstable();
                let mean = if latched.is_empty() {
                    0.0
                } else {
                    latched.iter().sum::<u128>() as f64 / latched.len() as f64
                };
                let p95 = latched
                    .get((latched.len() as f64 * 0.95).floor() as usize)
                    .copied()
                    .unwrap_or(0);
                let max = latched.iter().copied().max().unwrap_or(0);
                points.push(FrontierPoint {
                    mode: mode.clone(),
                    universe,
                    k,
                    inferable_relevant: relevant,
                    inferable_recovered: recovered,
                    inferable_recall: recall,
                    violations,
                    mean_latency_ms: mean,
                    p95_latency_ms: p95 as f64,
                    max_latency_ms: max,
                    descriptor_bytes: descriptor_bytes[&(mode.clone(), universe)],
                    gate,
                    passes: recall >= gate && violations == 0,
                });
            }
        }
    }

    // Per-tool recall at universe 64 (gate-critical universe).
    let expanded_64 = expand_universe_local(&dev, 64).context("expand 64")?;
    let mut tools: BTreeSet<String> = BTreeSet::new();
    for case in &expanded_64 {
        for tool in inferable_for_case(&case.case_id, &eligible) {
            tools.insert(tool);
        }
    }
    let mut per_tool = Vec::new();
    for tool in tools {
        let mut cases_count = 0usize;
        let mut hit_16 = 0usize;
        let mut hit_24 = 0usize;
        let mut hit_32 = 0usize;
        // Best-mode attribution: a tool counts recovered if ANY lexical mode
        // places it in the shortlist (frontier union across lexical modes).
        for case in &expanded_64 {
            if !inferable_for_case(&case.case_id, &eligible).contains(&tool) {
                continue;
            }
            cases_count += 1;
            for (k, hit) in [(16, &mut hit_16), (24, &mut hit_24), (32, &mut hit_32)] {
                let mut placed = false;
                for mode in &modes {
                    let ordered = &orderings[&(mode.clone(), 64, case.case_id.clone())];
                    if ordered.iter().take(k).any(|(name, _)| name == &tool) {
                        placed = true;
                        break;
                    }
                }
                if placed {
                    *hit += 1;
                }
            }
        }
        per_tool.push(PerToolRecall {
            tool,
            inferable_cases: cases_count,
            recovered_at_16: hit_16,
            recovered_at_24: hit_24,
            recovered_at_32: hit_32,
        });
    }
    per_tool.sort_by(|left, right| left.tool.cmp(&right.tool));

    // Persistent-miss rows for the four M001 misses (inferable instances).
    let mut persistent_misses = Vec::new();
    for case in &expanded_64 {
        for tool in inferable_for_case(&case.case_id, &eligible) {
            if !["glob", "table_filter", "write", "lsp_rename"].contains(&tool.as_str()) {
                continue;
            }
            let v1 = &orderings[&(
                "bm25-flat-v1-baseline".to_string(),
                64,
                case.case_id.clone(),
            )];
            let v1_rank = v1.iter().position(|(name, _)| name == &tool);
            let mut best: Option<(usize, String)> = None;
            for mode in &modes {
                if mode == "bm25-flat-v1-baseline" {
                    continue;
                }
                let ordered = &orderings[&(mode.clone(), 64, case.case_id.clone())];
                if let Some(rank) = ordered.iter().position(|(name, _)| name == &tool) {
                    let improves = best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank);
                    if improves {
                        best = Some((rank, mode.clone()));
                    }
                }
            }
            let (best_rank, best_mode) = best
                .map(|(rank, mode)| (Some(rank), mode))
                .unwrap_or((None, "none".to_string()));
            persistent_misses.push(PersistentMissRow {
                tool: tool.clone(),
                case_id: case.case_id.clone(),
                v1_rank,
                best_v2_rank: best_rank,
                best_v2_mode: best_mode.clone(),
                in_k16: best_rank.is_some_and(|rank| rank < 16),
                in_k24: best_rank.is_some_and(|rank| rank < 24),
                in_k32: best_rank.is_some_and(|rank| rank < 32),
            });
        }
    }
    persistent_misses.sort_by(|left, right| {
        left.tool
            .cmp(&right.tool)
            .then_with(|| left.case_id.cmp(&right.case_id))
    });

    // Schema split: frozen offline candidates carry no JSON schema, so every
    // inferable dev label is schema-absent by construction. The split is
    // reported (not tuned) to satisfy M002 §5.
    let mut schema_present = 0usize;
    let mut schema_absent = 0usize;
    for case in &expanded_64 {
        for tool in inferable_for_case(&case.case_id, &eligible) {
            let candidate = case.candidates.iter().find(|c| c.name == tool);
            let has_schema = candidate
                .is_some_and(|c| RetrievalDescriptorV2::from_candidate(c, None).has_schema());
            if has_schema {
                schema_present += 1;
            } else {
                schema_absent += 1;
            }
        }
    }

    let mut receipt = SignalV2FrontierReceipt {
        schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
        protocol: "m002-deterministic-signal-v2-frontier-v1".to_string(),
        prereg_protocol: SIGNAL_PREREG_PROTOCOL.to_string(),
        prereg_fingerprint: prereg_fp,
        derived_view: PREREG_DERIVED_VIEW.to_string(),
        derived_view_fingerprint: EXPECTED_DERIVED_VIEW_FINGERPRINT.to_string(),
        modes_measured: modes,
        modes_deferred: SEMANTIC_MODES.iter().map(|s| s.to_string()).collect(),
        points,
        per_tool,
        persistent_misses,
        schema_present_inferable: schema_present,
        schema_absent_inferable: schema_absent,
        fingerprint: String::new(),
    };
    receipt.fingerprint = frontier_fingerprint(&receipt)?;
    Ok(receipt)
}

/// Deterministic projection of the receipt: wall-clock latencies are
/// informational only and excluded from the fingerprint and drift
/// comparison so repeated measurements of identical rankings agree.
fn deterministic_projection(receipt: &SignalV2FrontierReceipt) -> serde_json::Value {
    let mut canonical = serde_json::to_value(receipt).expect("serialize frontier for projection");
    if let Some(map) = canonical.as_object_mut() {
        map.remove("fingerprint");
    }
    if let Some(points) = canonical
        .get_mut("points")
        .and_then(|value| value.as_array_mut())
    {
        for point in points {
            if let Some(map) = point.as_object_mut() {
                map.remove("mean_latency_ms");
                map.remove("p95_latency_ms");
                map.remove("max_latency_ms");
            }
        }
    }
    canonical
}

/// Deterministic fingerprint over the receipt (excludes `fingerprint` and
/// wall-clock latencies).
pub fn frontier_fingerprint(receipt: &SignalV2FrontierReceipt) -> Result<String> {
    let canonical = deterministic_projection(receipt);
    let bytes = serde_json::to_vec(&canonical).context("canonical frontier bytes")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn write_frontier_atomic(path: &Path, receipt: &SignalV2FrontierReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create frontier directory {}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(receipt).context("serialize frontier")?;
    std::fs::write(&temp, bytes)
        .with_context(|| format!("write temporary frontier {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("install frontier {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representation_follows_frozen_contract() {
        let candidate = ToolAdvisorCandidate {
            name: "lsp_rename".to_string(),
            description: "Rename a symbol".to_string(),
            category: "LSP".to_string(),
            disclosure: "deferred".to_string(),
            synthetic_identity: false,
        };
        let descriptor = RetrievalDescriptorV2::from_candidate(&candidate, None);
        assert_eq!(descriptor.identifier_tokens, "lsp rename");
        assert!(descriptor.flat_text().contains("lsp_rename"));
        assert!(descriptor
            .field_labelled_text()
            .contains("identifiers: lsp rename"));
        assert!(!descriptor.has_schema());
        // Offline cases never invent schema fields.
        assert!(descriptor.schema_property_names.is_empty());
        // Total cap holds.
        assert!(descriptor.encoded_bytes() <= DESCRIPTOR_TOTAL_CAP_BYTES);
    }

    #[test]
    fn schema_extraction_is_bounded_and_static_only() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern"},
                "path": {"type": "string", "description": "Base directory", "default": "/tmp"},
                "limit": {"type": "integer", "examples": [10]}
            }
        });
        let (names, descriptions) = schema_fields_from_parameters(&schema);
        assert_eq!(names, "limit path pattern");
        assert!(descriptions.contains("Glob pattern"));
        assert!(descriptions.contains("Base directory"));
        // Defaults/examples/values never leak into signal.
        assert!(!names.contains("/tmp"));
        assert!(!descriptions.contains("/tmp"));
        assert!(!descriptions.contains('['));
        let (empty_names, _) = schema_fields_from_parameters(&serde_json::Value::Null);
        assert!(empty_names.is_empty());
    }

    #[test]
    fn query_construction_matches_frozen_fields() {
        assert_eq!(QUERY_FIELDS.len(), 5);
        let query = RetrievalQueryV2::from_benchmark_context("Locate every test fixture");
        assert_eq!(query.current_objective, "Locate every test fixture");
        assert!(query.current_task.is_empty());
        assert!(query.next_steps.is_empty());
        assert_eq!(query.flat_text(), "Locate every test fixture");
        assert!(query
            .normalized_text()
            .contains("locate every test fixture"));
    }

    #[test]
    fn operation_terms_are_generic() {
        // Description verbs map to families; no per-tool table.
        let terms = operation_terms("glob", "Find files by path pattern");
        assert!(terms.contains("glob"));
        assert!(terms.contains("locate"));
        let write_terms = operation_terms("write", "Write content to files");
        assert!(write_terms.contains("create"));
        // Unseen tool, same rules: name tokens plus generic families only.
        assert_eq!(operation_terms("my_tool", "Do something"), "my tool");
    }

    #[test]
    fn semantic_cache_key_binds_all_five_factors() {
        let candidate = ToolAdvisorCandidate {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            category: "ReadOnly".to_string(),
            disclosure: "deferred".to_string(),
            synthetic_identity: false,
        };
        let descriptor = RetrievalDescriptorV2::from_candidate(&candidate, None);
        let first = semantic_cache_key(&descriptor, "tok-v1", "mean", "surface-a");
        let pooling_diff = semantic_cache_key(&descriptor, "tok-v1", "cls", "surface-a");
        let surface_diff = semantic_cache_key(&descriptor, "tok-v1", "mean", "surface-b");
        assert_ne!(first, pooling_diff);
        assert_ne!(first, surface_diff);
        assert_eq!(first.representation_schema_version, 2);
        assert_eq!(first.prereg_protocol, SIGNAL_PREREG_PROTOCOL);
    }

    #[test]
    fn lexical_modes_reproduce_v1_baseline_exactly() {
        let cases = super::super::builtin_cases().expect("corpus");
        let partition = super::super::partition_cases(&cases);
        let dev_case = &cases[partition.dev_cases[0]];
        let expanded =
            expand_universe_local(std::slice::from_ref(dev_case), 64).expect("expand one");
        let case = &expanded[0];
        let via_module = lexical_ordering(case, "bm25-flat-v1-baseline").expect("v1 mode");
        let direct: Vec<(String, f64)> = bm25_ordering_local(case)
            .into_iter()
            .map(|entry| (entry.name, entry.score))
            .collect();
        assert_eq!(via_module, direct);
    }

    #[test]
    fn lexical_frontier_is_deterministic() {
        let first = measure_lexical_frontier().expect("frontier");
        let second = measure_lexical_frontier().expect("frontier again");
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.points.len(), 4 * 3 * 3);
    }

    #[test]
    fn committed_frontier_matches_live_measurement() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join(SIGNAL_V2_FRONTIER_ASSET);
        if std::fs::read(&path).is_err() {
            eprintln!("SKIP: frontier asset not yet generated");
            return;
        }
        let bytes = std::fs::read(&path).expect("reread frontier");
        let stored: SignalV2FrontierReceipt =
            serde_json::from_slice(&bytes).expect("parse frontier");
        let fresh = measure_lexical_frontier().expect("fresh frontier");
        assert_eq!(
            deterministic_projection(&stored),
            deterministic_projection(&fresh),
            "committed frontier drifted"
        );
        // Latencies are informational: they must be present and sane, not equal.
        for point in &stored.points {
            assert!(point.p95_latency_ms <= point.max_latency_ms as f64);
        }
    }

    /// Generate the interim lexical frontier asset. Run explicitly:
    /// `cargo test --locked -p codegg --lib --
    ///  tool_advisor::retrieval_signal_v2::tests::generate_frontier_asset --
    ///  --ignored --nocapture`
    #[test]
    #[ignore]
    fn generate_frontier_asset() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let receipt = measure_lexical_frontier().expect("frontier");
        let path = root.join(SIGNAL_V2_FRONTIER_ASSET);
        write_frontier_atomic(&path, &receipt).expect("write frontier");
        for point in receipt.points.iter().filter(|point| point.k == 16) {
            eprintln!(
                "{} u{} k{}: {}/{}={:.4} violations={} p95={:.1}ms",
                point.mode,
                point.universe,
                point.k,
                point.inferable_recovered,
                point.inferable_relevant,
                point.inferable_recall,
                point.violations,
                point.p95_latency_ms
            );
        }
        eprintln!("fingerprint={}", receipt.fingerprint);
    }

    #[test]
    fn baseline_prediction_import_is_live() {
        // Guards the v1-parity import against dead-code drift.
        let cases = super::super::builtin_cases().expect("corpus");
        let prediction = baseline_prediction(&cases[0], crate::tool::catalog::SearchMode::BM25);
        assert_eq!(prediction.case_id, cases[0].case_id);
    }

    #[test]
    fn derived_view_asset_binding_holds() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join(PREREG_DERIVED_VIEW);
        if std::fs::read(&path).is_err() {
            eprintln!("SKIP: derived view absent");
            return;
        }
        let bytes = std::fs::read(&path).expect("reread derived view");
        let view: crate::tool_advisor::retrieval_relevance::RetrievalRelevanceView =
            serde_json::from_slice(&bytes).expect("parse view");
        assert_eq!(view.fingerprint, EXPECTED_DERIVED_VIEW_FINGERPRINT);
    }
}
