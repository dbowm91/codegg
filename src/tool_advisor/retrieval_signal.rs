//! M001 retrieval-signal experiment: signal-sufficiency audit and preregistration.
//!
//! This module answers two questions before any scoring change:
//!
//! 1. Is each gate-critical relevant label inferable from the bounded
//!    retrieval query ([`AdvisorContextV2`] state only)?
//! 2. If yes, what representation signal is absent or misaligned in the
//!    current v1 (name + description) retrieval text?
//!
//! No retriever/model training occurs here. The module freezes the
//! Retrieval Signal V2 representation contract and the conditional M003
//! learned-projection search space, and emits the compact
//! preregistration/diagnostic receipt consumed by M002/M003.
//!
//! Design notes:
//!
//! - Everything in this module is deterministic and dependency-free (no
//!   encoder weights, no subprocesses, no network). BM25 ranks are derived
//!   live from [`baseline_prediction`]; raw BM25 scores use an M001-local
//!   mirror of the catalog formula (`crate::tool::catalog`, private
//!   `tokenize`/`bm25_score`) marked as such, because the public
//!   `rank_descriptors` API returns order only.
//! - Universe expansion mirrors
//!   `crate::tool_advisor::operating_point::expand_universe` exactly
//!   (preserve relevant re-homed to deferred, deterministic
//!   `fixture_deferred_{NNN}` filler, name-sorted, validated). The
//!   operating-point module is feature-gated behind
//!   `tool-advisor-encoder-training`, which does not compile on every
//!   host; this mirror keeps the M001 audit runnable everywhere. Any
//!   divergence fails the reproduction tripwire test instead of silently
//!   rebinding the audit.
//! - Semantic/fused ranks for the four persistent misses are quoted from
//!   the frozen retrieval-architecture M003 closure (commit `fb11a74f`)
//!   with explicit provenance, because running the MiniLM encoder is out
//!   of scope for M001 and the weights are not vendored in this host.
//!   They are labeled `quoted-frozen`, never `measured-live`.

use super::context_v2::AdvisorContextV2;
use super::{
    baseline_prediction, dataset_fingerprint, load_cases, normalize_text, partition_cases,
    ToolAdvisorCandidate, ToolAdvisorCase,
};
use crate::tool::catalog::SearchMode;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

// ---- frozen protocol identity ----

/// Versioned representation contract frozen by M001.
pub const RETRIEVAL_SIGNAL_SCHEMA_VERSION: u16 = 2;
/// Preregistration protocol name for this experiment stage.
pub const RETRIEVAL_SIGNAL_M001_PROTOCOL: &str = "retrieval-signal-m001-preregistration-v1";
/// Code version of the M001-local universe-expansion mirror. Any change to
/// the expansion logic must bump this so the fixture fingerprint mismatches
/// instead of silently rebinding the audit.
pub const M001_EXPANSION_CODE_VERSION: u16 = 1;

// ---- frozen gates (unchanged from M004) ----

/// Recall gates preserved without relaxation through M001-M004.
pub const GATE_RECALL_64: f64 = 0.99;
pub const GATE_RECALL_128: f64 = 0.98;
pub const GATE_RECALL_256: f64 = 0.95;
/// Candidate universes (deferred tools per case) for the dev frontier.
pub const PREREG_UNIVERSES: [usize; 3] = [64, 128, 256];
/// Primary shortlists. Gates must clear at K<=32.
pub const PREREG_PRIMARY_KS: [usize; 3] = [16, 24, 32];

// ---- frozen M004 reference counts (reproduction tripwire) ----

/// M004 committed reference: BM25 recovered tools per universe (flat across
/// K=16/24/32). Recorded at `retrieval_architecture::M004_BM25_RECOVERED`.
pub const M004_BM25_RECOVERED: usize = 52;
/// M004 committed reference: eligible relevant tools per universe.
pub const M004_ELIGIBLE_RELEVANT: usize = 72;
/// M004 committed reference: dev cases per universe.
pub const M004_DEV_CASES: usize = 62;
/// Expected dev-partition fingerprint of the frozen corpus under
/// [`partition_cases`]. Any corpus or partition-logic drift fails closed.
pub const EXPECTED_DEV_PARTITION_FINGERPRINT: &str =
    "b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9";

// ---- frozen query/descriptor field contract ----

/// Frozen query field order for Retrieval Signal V2. The order is part of
/// the contract: reordering fields changes the preregistration hash.
pub const QUERY_FIELD_ORDER: [&str; 5] = [
    "current_objective",
    "current_task",
    "next_steps",
    "unresolved_signal",
    "capability_cue",
];
/// Per-field byte cap inherited from [`AdvisorContextV2`].
pub const QUERY_FIELD_BYTE_CAP: usize = 2 * 1024;
/// Total serialized query cap inherited from [`AdvisorContextV2`].
pub const QUERY_TOTAL_BYTE_CAP: usize = 8 * 1024;

/// Frozen descriptor field order for Retrieval Signal V2.
pub const DESCRIPTOR_FIELD_ORDER: [&str; 6] = [
    "canonical_name",
    "identifier_tokens",
    "description",
    "category_disclosure",
    "schema_fields",
    "normalization",
];
/// Total byte cap for extracted parameter-schema text per candidate.
pub const DESCRIPTOR_SCHEMA_BYTE_CAP: usize = 1024;
/// Per-parameter description cap inside schema extraction.
pub const DESCRIPTOR_SCHEMA_FIELD_DESC_CAP: usize = 256;

// ---- frozen deterministic variant grid (M002 degrees of freedom) ----

/// Preregistered deterministic lexical variants. Weights are fixed here;
/// M002 must not tune new weights after measurement.
pub const DETERMINISTIC_LEXICAL_VARIANTS: [&str; 4] = [
    "v1-bm25-baseline",
    "signal-v2-flat-bm25",
    "signal-v2-field-weighted-bm25",
    "signal-v2-normalized-bm25",
];
/// Preregistered deterministic semantic descriptor/query encodings. No
/// encoder/projection weight is trained in M002.
pub const DETERMINISTIC_SEMANTIC_VARIANTS: [&str; 3] = [
    "signal-v2-flat",
    "signal-v2-field-labelled-descriptor",
    "signal-v2-field-labelled-query",
];
/// Preregistered pooling options for deterministic semantic encodings.
pub const DETERMINISTIC_POOLING_VARIANTS: [&str; 2] = ["mean", "cls"];

// ---- frozen conditional learned grid (M003 degrees of freedom) ----

/// Maximum trainable projection parameters for conditional M003.
pub const MAX_PROJECTION_PARAMS: u64 = 500_000;
/// Preregistered projection families over the frozen pinned MiniLM (384-dim).
pub const PROJECTION_FAMILIES: [&str; 3] = [
    "shared-linear-384x128",
    "asymmetric-linear-384x128",
    "asymmetric-mlp-384x128x128",
];
/// Preregistered learning-rate grid.
pub const PROJECTION_LR_GRID: [f64; 3] = [1e-4, 3e-4, 1e-3];
/// Epoch bound for projection optimization.
pub const PROJECTION_MAX_EPOCHS: u32 = 5;
/// Preregistered contrastive temperature grid.
pub const PROJECTION_TEMPERATURE_GRID: [f64; 3] = [0.05, 0.07, 0.1];
/// Preregistered random seeds.
pub const PROJECTION_SEEDS: [u64; 3] = [7, 21, 42];
/// Hard negatives mined per positive (train-only, frozen pre-M003 scorers).
pub const PROJECTION_HARD_NEGATIVES_PER_POSITIVE: usize = 7;
/// Preregistered batch size.
pub const PROJECTION_BATCH_SIZE: usize = 64;

// ---- generic normalization (preregistered, tool-agnostic) ----

/// No hand-authored per-tool aliases are registered. Any lexical
/// normalization in Signal V2 is limited to the generic operations
/// implemented here: identifier decomposition, lowercase Unicode
/// normalization, punctuation separation, and conservative morphology
/// shared across all tools. This empty lexicon is itself preregistered:
/// M002 must not add synonyms after inspecting misses.
pub const PREREGISTERED_SYNONYM_LEXICON_ENTRIES: usize = 0;

// ---- persistent-miss tool scope ----

/// The four dual-signal-invisible tools from the closed fusion experiment.
pub const PERSISTENT_MISS_TOOLS: [&str; 4] = ["glob", "table_filter", "write", "lsp_rename"];

// ---- identifier decomposition ----

/// Split a canonical tool identifier into retrieval tokens.
///
/// Splits on snake_case, kebab-case, punctuation, and camel-case
/// boundaries, lowercases, drops empties, and dedupes preserving first
/// occurrence order. Deterministic across runs and hosts.
pub fn split_identifier_tokens(name: &str) -> Vec<String> {
    let mut raw_parts = Vec::new();
    let mut current = String::new();
    let mut chars = name.chars().peekable();
    let mut prev_was_lower = false;
    while let Some(ch) = chars.next() {
        if ch.is_alphanumeric() {
            let is_upper = ch.is_uppercase();
            // Camel boundary: lower/digit followed by upper starts a token.
            if is_upper
                && (prev_was_lower
                    || (!current.is_empty()
                        && current.chars().last().is_some_and(|c| c.is_numeric())))
            {
                if !current.is_empty() {
                    raw_parts.push(std::mem::take(&mut current));
                }
            }
            current.extend(ch.to_lowercase());
            prev_was_lower = !is_upper && ch.is_alphabetic();
            if ch.is_numeric() {
                prev_was_lower = false;
            }
        } else {
            if !current.is_empty() {
                raw_parts.push(std::mem::take(&mut current));
            }
            prev_was_lower = false;
        }
        let _ = chars.peek();
    }
    if !current.is_empty() {
        raw_parts.push(current);
    }
    let mut seen = BTreeSet::new();
    raw_parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .filter(|part| seen.insert(part.clone()))
        .collect()
}

/// Generic signal tokens for a free-text span: Unicode-normalized
/// lowercasetokenization plus identifier decomposition of each token.
/// Shared across all tools; no per-tool lexicon.
pub fn signal_tokens(text: &str) -> Vec<String> {
    let normalized = normalize_text(text);
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();
    for piece in normalized.split_whitespace() {
        for token in split_identifier_tokens(piece) {
            if seen.insert(token.clone()) {
                tokens.push(token);
            }
        }
    }
    tokens
}

// ---- parameter-schema extraction ----

/// One bounded, deterministically ordered schema field extracted from a
/// live input schema. Carries names/types/descriptions only: examples,
/// defaults, const/enum payloads, endpoints, credentials, and runtime
/// values are never extracted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
    pub description: String,
    pub required: bool,
}

/// Extract bounded schema fields from a JSON-Schema input schema.
///
/// - reads `properties` (object) plus `required` (array);
/// - sorts fields by name for determinism;
/// - keeps only name, type (as a plain string), description (capped), and
///   the required marker;
/// - never emits `example(s)`, `default`, `const`, `enum`, or any other
///   value-bearing keyword that could carry secrets or runtime data;
/// - caps total serialized bytes at [`DESCRIPTOR_SCHEMA_BYTE_CAP`];
/// - returns an empty vec for candidates without a schema (still valid).
pub fn extract_schema_fields(schema: &serde_json::Value) -> Vec<SchemaField> {
    let properties = schema.get("properties").and_then(|value| value.as_object());
    let Some(properties) = properties else {
        return Vec::new();
    };
    let required: BTreeSet<&str> = schema
        .get("required")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default();
    let mut fields = Vec::new();
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort();
    for name in names {
        let property = &properties[name];
        let field_type = match property.get("type") {
            Some(serde_json::Value::String(text)) => truncate_to_char_boundary(text, 64),
            Some(other) => {
                truncate_to_char_boundary(&serde_json::to_string(other).unwrap_or_default(), 64)
            }
            None => String::new(),
        };
        let description = property
            .get("description")
            .and_then(|value| value.as_str())
            .map(|text| truncate_to_char_boundary(text, DESCRIPTOR_SCHEMA_FIELD_DESC_CAP))
            .unwrap_or_default();
        fields.push(SchemaField {
            name: truncate_to_char_boundary(name, 128),
            field_type,
            description,
            required: required.contains(name.as_str()),
        });
    }
    // Enforce the total byte cap by dropping trailing fields (names are
    // sorted, so truncation is deterministic).
    while schema_fields_bytes(&fields) > DESCRIPTOR_SCHEMA_BYTE_CAP && !fields.is_empty() {
        fields.pop();
    }
    fields
}

fn schema_fields_bytes(fields: &[SchemaField]) -> usize {
    fields
        .iter()
        .map(|field| field.name.len() + field.field_type.len() + field.description.len() + 8)
        .sum()
}

fn truncate_to_char_boundary(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

// ---- live schema plumbing audit ----

/// Frozen offline copies of the live input schemas for the two
/// persistent-miss tools that map to standalone built-ins.
///
/// Provenance: `src/tool/glob.rs` (`GlobTool::parameters`) and
/// `src/tool/write.rs` (`WriteTool::parameters`). Captured as literals
/// (rather than instantiating the tools, whose constructors read
/// `std::env::current_dir()`) so the audit stays host-independent.
/// Any live schema change must update these literals and the
/// preregistration hash together.
pub fn live_builtin_schema(name: &str) -> Option<serde_json::Value> {
    match name {
        "glob" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern to match"},
                "path": {"type": "string", "description": "Directory to search in (default: current directory)"}
            },
            "required": ["pattern"]
        })),
        "write" => Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file to write"},
                "content": {"type": "string", "description": "Content to write to the file"}
            },
            "required": ["path", "content"]
        })),
        _ => None,
    }
}

/// Schema-plumbing audit finding (M001 §6).
///
/// Source seam: `ResolvedToolSurface` (`src/agent/tool_surface.rs`)
/// carries `ResolvedTool.definition: ToolDefinition`
/// (`crates/codegg-providers/src/provider_core.rs`), whose `parameters:
/// serde_json::Value` is the live input schema. `ToolAdvisorCandidate`
/// (`src/tool_advisor/mod.rs`) drops it at all three construction sites
/// (`from_metadata`, `candidates_from_deferred_surface`,
/// `baseline_prediction`/`preselect_candidates` via
/// `parameters: Null`).
///
/// Disposition: prefer the experiment-local [`RetrievalDescriptorV2`]
/// over expanding the durable advisor artifact. No `ToolAdvisorCandidate`
/// schema change occurs in M001.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaPlumbingAudit {
    pub source_type: String,
    pub source_path: String,
    pub candidate_needs_optional_field: bool,
    pub experiment_local_representation_sufficient: bool,
    pub builtin_with_schema: Vec<String>,
    pub without_schema: Vec<String>,
    pub finding: String,
}

pub fn schema_plumbing_audit() -> SchemaPlumbingAudit {
    SchemaPlumbingAudit {
        source_type: "ResolvedToolSurface -> ResolvedTool.definition: ToolDefinition { parameters: serde_json::Value }".into(),
        source_path: "src/agent/tool_surface.rs + crates/codegg-providers/src/provider_core.rs".into(),
        candidate_needs_optional_field: false,
        experiment_local_representation_sufficient: true,
        builtin_with_schema: vec!["glob".into(), "write".into()],
        without_schema: vec![
            "table_filter".into(),
            "lsp_rename".into(),
            "fixture_deferred_*".into(),
        ],
        finding: "table_filter has no live Tool impl in src/ (benchmark-only candidate name); lsp_rename has no standalone live Tool impl (live LSP surface exposes operation enums, not this candidate name). Both degrade to name/description/category/disclosure in Signal V2 and are reported in the schema-absent slice. glob/write map to GlobTool/WriteTool schemas captured above.".into(),
    }
}

// ---- Retrieval Signal V2 representation ----

/// Versioned retrieval query built only from allowed
/// [`AdvisorContextV2`] fields in frozen [`QUERY_FIELD_ORDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalQueryV2 {
    pub schema_version: u16,
    pub current_objective: Option<String>,
    pub current_task: Option<String>,
    pub next_steps: Vec<String>,
    pub unresolved_signal: Option<String>,
    pub capability_cue: Option<String>,
}

impl RetrievalQueryV2 {
    pub fn from_advisor_context(context: &AdvisorContextV2) -> Self {
        Self {
            schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
            current_objective: context.current_objective.clone(),
            current_task: context.current_task.clone(),
            next_steps: context.next_steps.clone(),
            unresolved_signal: context.unresolved_signal.clone(),
            capability_cue: context.capability_cue.clone(),
        }
    }

    /// Deterministic serialization in frozen field order with per-field
    /// and total byte caps. Carries only the five allowed fields; never
    /// transcript history, tool arguments/results, or secrets (inherited
    /// from [`AdvisorContextV2`]'s constructor).
    pub fn serialize(&self) -> String {
        let mut output = format!("retrieval-signal-v{RETRIEVAL_SIGNAL_SCHEMA_VERSION}\n");
        let fields: [(&str, Option<String>); 5] = [
            ("current_objective", self.current_objective.clone()),
            ("current_task", self.current_task.clone()),
            (
                "next_steps",
                (!self.next_steps.is_empty()).then(|| self.next_steps.join(" | ")),
            ),
            ("unresolved_signal", self.unresolved_signal.clone()),
            ("capability_cue", self.capability_cue.clone()),
        ];
        for (label, value) in fields {
            if let Some(value) = value {
                let bounded = truncate_to_char_boundary(&value, QUERY_FIELD_BYTE_CAP);
                let line = format!("{label}: {bounded}\n");
                if output.len() + line.len() <= QUERY_TOTAL_BYTE_CAP {
                    output.push_str(&line);
                }
            }
        }
        truncate_to_char_boundary(&output, QUERY_TOTAL_BYTE_CAP)
    }

    /// Generic retrieval tokens for the query (audit/diagnostic helper).
    pub fn tokens(&self) -> Vec<String> {
        signal_tokens(&self.serialize())
    }
}

/// Versioned retrieval descriptor built only from allowed static tool
/// metadata in frozen [`DESCRIPTOR_FIELD_ORDER`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalDescriptorV2 {
    pub schema_version: u16,
    pub canonical_name: String,
    pub identifier_tokens: Vec<String>,
    pub description: String,
    pub category: String,
    pub disclosure: String,
    pub schema_fields: Vec<SchemaField>,
}

impl RetrievalDescriptorV2 {
    pub fn from_candidate(
        candidate: &ToolAdvisorCandidate,
        schema_fields: Vec<SchemaField>,
    ) -> Self {
        Self {
            schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
            canonical_name: candidate.name.clone(),
            identifier_tokens: split_identifier_tokens(&candidate.name),
            description: candidate.description.clone(),
            category: candidate.category.clone(),
            disclosure: candidate.disclosure.clone(),
            schema_fields,
        }
    }

    fn schema_text(&self) -> String {
        self.schema_fields
            .iter()
            .map(|field| {
                format!(
                    "{}:{}:{}{}",
                    field.name,
                    field.field_type,
                    field.description,
                    if field.required {
                        ":required"
                    } else {
                        ":optional"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Flat descriptor text for the `signal-v2-flat(-bm25)` variants.
    pub fn flat_text(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.canonical_name,
            self.identifier_tokens.join(" "),
            self.description,
            self.category,
            self.disclosure,
            self.schema_text()
        )
    }

    /// Field-labelled descriptor text for the
    /// `signal-v2-field-labelled-descriptor` variant.
    pub fn field_labelled_text(&self) -> String {
        format!(
            "name: {}; id: {}; description: {}; category: {}; disclosure: {}; schema: {}",
            self.canonical_name,
            self.identifier_tokens.join(" "),
            self.description,
            self.category,
            self.disclosure,
            self.schema_text()
        )
    }
}

/// Descriptor embedding cache key for Signal V2. Contains descriptor
/// identity only: representation schema/version, descriptor fingerprint,
/// encoder/tokenizer hash, pooling, and surface fingerprint. Never user
/// context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalCacheKeyV2 {
    pub representation_schema_version: u16,
    pub descriptor_fingerprint: String,
    pub encoder_tokenizer_version: String,
    pub pooling: String,
    pub surface_fingerprint: String,
}

pub fn descriptor_fingerprint_v2(descriptor: &RetrievalDescriptorV2) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"retrieval-signal-v2-descriptor-v1\n");
    hasher.update(descriptor.canonical_name.as_bytes());
    hasher.update(b"\n");
    hasher.update(descriptor.flat_text().as_bytes());
    hex::encode(hasher.finalize())
}

// ---- local BM25 score mirror (attribution only) ----

fn catalog_tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Raw BM25 score mirror of `crate::tool::catalog` (`k1=1.5, b=0.75`,
/// `name + description` documents). Attribution-only: ranks always come
/// from the live [`baseline_prediction`] ordering.
fn mirrored_bm25_score(
    query: &str,
    document: &str,
    avg_dl: f64,
    idf: &BTreeMap<String, f64>,
) -> f64 {
    use std::collections::HashMap;
    let k1 = 1.5;
    let b = 0.75;
    let query_terms = catalog_tokenize(query);
    let doc_terms = catalog_tokenize(document);
    let doc_len = doc_terms.len() as f64;
    let mut tf: HashMap<String, usize> = HashMap::new();
    for term in &doc_terms {
        *tf.entry(term.clone()).or_insert(0) += 1;
    }
    let mut score = 0.0;
    for term in &query_terms {
        if let Some(&term_tf) = tf.get(term) {
            let idf_val = idf.get(term).copied().unwrap_or(0.0);
            let numerator = term_tf as f64 * (k1 + 1.0);
            let denominator = term_tf as f64 + k1 * (1.0 - b + b * doc_len / avg_dl);
            score += idf_val * numerator / denominator;
        }
    }
    score
}

fn mirrored_idf(documents: &[String]) -> (BTreeMap<String, f64>, f64) {
    use std::collections::{HashMap, HashSet};
    let n = documents.len() as f64;
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    let mut total_len = 0usize;
    for doc in documents {
        let terms = catalog_tokenize(doc);
        total_len += terms.len();
        for term in terms.into_iter().collect::<HashSet<_>>() {
            *doc_freq.entry(term).or_insert(0) += 1;
        }
    }
    let avg_dl = if documents.is_empty() {
        0.0
    } else {
        total_len as f64 / documents.len() as f64
    };
    let idf = doc_freq
        .into_iter()
        .map(|(term, df)| {
            let value = ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
            (term, value)
        })
        .collect();
    (idf, avg_dl)
}

// ---- universe expansion mirror ----

/// M001-local mirror of `operating_point::expand_universe`: expand dev
/// cases to exactly `size` deferred candidates. Relevant labels are
/// preserved (re-homed to deferred when needed); the remainder is
/// deterministic `fixture_deferred_{NNN}` filler; output is name-sorted
/// and validated.
pub fn expand_universe_m001(
    cases: &[ToolAdvisorCase],
    size: usize,
) -> Result<Vec<ToolAdvisorCase>> {
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
    validate_universe_m001(&expanded, size)?;
    Ok(expanded)
}

fn validate_universe_m001(cases: &[ToolAdvisorCase], expected_universe_size: usize) -> Result<()> {
    for case in cases {
        let deferred: Vec<_> = case
            .candidates
            .iter()
            .filter(|candidate| candidate.disclosure == "deferred")
            .collect();
        if deferred.len() != expected_universe_size {
            return Err(anyhow!(
                "fixture case {} has {} deferred candidates; expected universe {expected_universe_size}",
                case.case_id,
                deferred.len()
            ));
        }
        if !case.none {
            let missing: Vec<_> = case
                .relevance
                .keys()
                .filter(|name| !deferred.iter().any(|candidate| &candidate.name == *name))
                .collect();
            if !missing.is_empty() {
                return Err(anyhow!(
                    "fixture case {} lost relevant tools during universe expansion",
                    case.case_id
                ));
            }
        }
    }
    Ok(())
}

// ---- live BM25 ordering (rank evidence) ----

/// Full live BM25 ordering over the deferred-eligible candidates of one
/// case, mirroring `sequence_retrieval::bm25_order` without K truncation:
/// [`baseline_prediction`] order filtered to eligible, zero-score
/// zero-completion, deterministic score-desc/name-asc tiebreak.
pub fn bm25_full_order(case: &ToolAdvisorCase) -> Vec<(String, f64)> {
    let prediction = baseline_prediction(case, SearchMode::BM25);
    let allowed = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.clone())
        .collect::<BTreeSet<_>>();
    let mut ranked: Vec<(String, f64)> = prediction
        .ranked
        .into_iter()
        .filter(|candidate| allowed.contains(&candidate.name))
        .map(|candidate| (candidate.name, candidate.score))
        .collect();
    let known: BTreeSet<String> = ranked.iter().map(|(name, _)| name.clone()).collect();
    for candidate in case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
    {
        if !known.contains(&candidate.name) {
            ranked.push((candidate.name.clone(), 0.0));
        }
    }
    ranked.sort_by(|left, right| left.0.cmp(&right.0));
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked
}

/// Lexical overlap terms between the case query and one candidate's v1
/// retrieval text (`name + description`), using catalog tokenization.
pub fn lexical_overlap_terms(
    case: &ToolAdvisorCase,
    candidate: &ToolAdvisorCandidate,
) -> Vec<String> {
    let query_terms: BTreeSet<String> = catalog_tokenize(&case.context).into_iter().collect();
    let doc_terms: BTreeSet<String> =
        catalog_tokenize(&format!("{} {}", candidate.name, candidate.description))
            .into_iter()
            .collect();
    query_terms.intersection(&doc_terms).cloned().collect()
}

// ---- inferability taxonomy ----

/// Primary-cause classification for one missed relevant occurrence.
/// Exactly one variant applies; the receipt records the rationale plus
/// the exact allowed text supporting inferability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InferabilityClass {
    QueryExplicit,
    QueryParaphrase,
    DescriptorIncomplete,
    QueryProjectionLoss,
    ImplicitSecondary,
    OtherEvidenceDefect,
}

impl InferabilityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueryExplicit => "query-explicit",
            Self::QueryParaphrase => "query-paraphrase",
            Self::DescriptorIncomplete => "descriptor-incomplete",
            Self::QueryProjectionLoss => "query-projection-loss",
            Self::ImplicitSecondary => "implicit-secondary",
            Self::OtherEvidenceDefect => "other-evidence-defect",
        }
    }
}

/// Generic audit-only verb groups (analysis aid, NOT retrieval signal).
/// Each group links a descriptor verb to generic paraphrases observed in
/// task language. This table is frozen here and must not leak into the
/// Signal V2 scorer as per-tool aliases.
const PARAPHRASE_GROUPS: [(&str, &[&str]); 4] = [
    ("find", &["locate", "search", "look", "discover"]),
    ("write", &["create", "persist", "save", "make"]),
    ("rename", &["renaming", "renamed"]),
    ("filter", &["filtering", "filtered", "narrow", "select"]),
];

/// Workflow-deferral markers suggesting a supporting-step label rather
/// than a current-step tool need.
const DEFERRAL_MARKERS: [&str; 5] = [
    "already landed",
    "already handled",
    "trail is stale",
    "instead",
    "later review shows",
];

/// Deterministic inferability classifier over allowed query state.
///
/// Inputs are the normalized query token set, the candidate descriptor
/// (name/description/schema cue tokens), the relevance grade, whether it
/// is the case's highest grade, and the raw query text for marker scans.
/// Returns the single primary class plus a rationale and the exact
/// supporting substring from allowed query text.
pub fn classify_inferability(
    query_text: &str,
    candidate_name: &str,
    candidate_description: &str,
    schema_cue_tokens: &[String],
    grade: u8,
    is_highest_grade: bool,
) -> (InferabilityClass, String, String) {
    let query_tokens: BTreeSet<String> = signal_tokens(query_text).into_iter().collect();
    let name_tokens = split_identifier_tokens(candidate_name);
    let desc_tokens = signal_tokens(candidate_description);
    let query_lower = query_text.to_ascii_lowercase();

    // 1. query-explicit: canonical name token or a description content
    // phrase is directly expressed in the query.
    for token in &name_tokens {
        if token.len() >= 3 && query_tokens.contains(token) {
            return (
                InferabilityClass::QueryExplicit,
                format!("query directly expresses canonical identifier token '{token}'"),
                supporting_substring(query_text, token),
            );
        }
    }
    let desc_phrases = adjacent_phrases(&desc_tokens, 2);
    for phrase in &desc_phrases {
        let joined = phrase.join(" ");
        if query_lower.contains(&joined) && joined.len() >= 6 {
            return (
                InferabilityClass::QueryExplicit,
                format!("query directly expresses descriptor phrase '{joined}'"),
                supporting_substring(query_text, &joined),
            );
        }
    }
    for token in &desc_tokens {
        if token.len() >= 5 && query_tokens.contains(token) {
            return (
                InferabilityClass::QueryExplicit,
                format!("query directly expresses descriptor term '{token}'"),
                supporting_substring(query_text, token),
            );
        }
    }

    // 2. descriptor-incomplete: no name/description cue, but a live
    // schema field-name token appears in the query. The cue exists in the
    // tool contract but is absent from v1 retrieval text.
    for token in schema_cue_tokens {
        if token.len() >= 3 && query_tokens.contains(token) {
            return (
                InferabilityClass::DescriptorIncomplete,
                format!(
                    "query expresses schema cue '{token}' present in the live tool contract but absent from v1 retrieval text"
                ),
                supporting_substring(query_text, token),
            );
        }
    }

    // 3. query-paraphrase: a generic paraphrase of a descriptor verb.
    for token in &desc_tokens {
        if let Some(group) = paraphrase_group_for(token) {
            for paraphrase in group {
                if query_tokens.contains(*paraphrase) {
                    return (
                        InferabilityClass::QueryParaphrase,
                        format!(
                            "query expresses '{paraphrase}', a generic paraphrase of descriptor verb '{token}' with no exact descriptor term present"
                        ),
                        supporting_substring(query_text, paraphrase),
                    );
                }
            }
        }
    }

    // 4. implicit-secondary: secondary grade with no inferable cue plus
    // workflow-deferral language in the query.
    if !is_highest_grade
        && DEFERRAL_MARKERS
            .iter()
            .any(|marker| query_lower.contains(marker))
    {
        let marker = DEFERRAL_MARKERS
            .iter()
            .find(|marker| query_lower.contains(*marker))
            .unwrap_or(&DEFERRAL_MARKERS[0]);
        return (
            InferabilityClass::ImplicitSecondary,
            format!(
                "grade {grade} is not the case-highest grade, no descriptor/schema cue is expressed, and the query carries workflow-deferral language ('{marker}')"
            ),
            supporting_substring(query_text, marker),
        );
    }

    // 5. query-projection-loss is unreachable for benchmark cases:
    // from_benchmark_context preserves the whole fixture context in
    // current_objective, so no available cue is dropped. It remains a
    // live-path variant only and is never emitted here.
    if !is_highest_grade {
        return (
            InferabilityClass::ImplicitSecondary,
            format!(
                "grade {grade} is not the case-highest grade and no descriptor, schema, or paraphrase cue is expressed in allowed query state"
            ),
            query_text.chars().take(160).collect(),
        );
    }

    // 6. Primary-grade label with no cue whatsoever: fixture mismatch.
    (
        InferabilityClass::OtherEvidenceDefect,
        format!(
            "grade {grade} is the case-highest grade yet no descriptor, schema, or paraphrase cue is expressed in allowed query state"
        ),
        query_text.chars().take(160).collect(),
    )
}

fn adjacent_phrases(tokens: &[String], width: usize) -> Vec<Vec<String>> {
    let mut phrases = Vec::new();
    for window in tokens.windows(width) {
        phrases.push(window.to_vec());
    }
    phrases
}

/// Query-explicit support check shared by the classifier and the
/// sibling-aware secondary adjudication below. Returns the supporting
/// substring when the query directly expresses the candidate's
/// identifier token or descriptor phrase/term.
pub fn query_explicit_support(
    query_text: &str,
    candidate_name: &str,
    candidate_description: &str,
) -> Option<String> {
    let query_tokens: BTreeSet<String> = signal_tokens(query_text).into_iter().collect();
    let query_lower = query_text.to_ascii_lowercase();
    for token in split_identifier_tokens(candidate_name) {
        if token.len() >= 3 && query_tokens.contains(&token) {
            return Some(supporting_substring(query_text, &token));
        }
    }
    for phrase in adjacent_phrases(&signal_tokens(candidate_description), 2) {
        let joined = phrase.join(" ");
        if joined.len() >= 6 && query_lower.contains(&joined) {
            return Some(supporting_substring(query_text, &joined));
        }
    }
    for token in signal_tokens(candidate_description) {
        if token.len() >= 5 && query_tokens.contains(&token) {
            return Some(supporting_substring(query_text, &token));
        }
    }
    None
}

fn paraphrase_group_for(token: &str) -> Option<&'static [&'static str]> {
    for (verb, group) in PARAPHRASE_GROUPS {
        if token == verb {
            return Some(group);
        }
    }
    None
}

fn supporting_substring(query_text: &str, needle: &str) -> String {
    let lower = query_text.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if let Some(start) = lower.find(&needle_lower) {
        let end = (start + needle.len().max(24)).min(query_text.len());
        let mut end = end;
        while end < query_text.len() && !query_text.is_char_boundary(end) {
            end += 1;
        }
        let mut start = start;
        while start > 0 && !query_text.is_char_boundary(start) {
            start -= 1;
        }
        query_text[start..end].to_string()
    } else {
        query_text.chars().take(160).collect()
    }
}

// ---- miss audit ----

/// One audited missed relevant occurrence of a persistent-miss tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissOccurrence {
    pub tool: String,
    pub case_id: String,
    pub partition: String,
    pub relevance_grade: u8,
    pub preferred_order_position: Option<usize>,
    pub is_primary_highest_grade: bool,
    pub serialized_query: String,
    pub candidate_name: String,
    pub candidate_description: String,
    pub candidate_category: String,
    pub candidate_disclosure: String,
    pub live_schema_fields: Vec<SchemaField>,
    pub bm25_rank: usize,
    pub bm25_score_model: String,
    pub bm25_score: f64,
    pub semantic_rank_quoted: Option<usize>,
    pub fused_rank_quoted: Option<usize>,
    pub fused_margin_quoted: Option<f64>,
    pub lexical_overlap_terms: Vec<String>,
    pub nearest_wrong_candidates: Vec<String>,
    pub inferability: String,
    pub inferability_rationale: String,
    pub inferability_support: String,
}

/// Frozen quoted semantic/fused evidence from the closed M003 sweep
/// (commit `fb11a74f`, u256 K=32, best fusion families). Ranks are
/// occurrence-aggregated per tool: every missed occurrence of the tool
/// sat at or near the bottom of both orderings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotedFusionEvidence {
    pub tool: String,
    pub bm25_rank: usize,
    pub semantic_rank: usize,
    pub fused_rank: usize,
    pub fused_margin: f64,
    pub cause: String,
    pub provenance: String,
}

pub fn quoted_m003_evidence() -> Vec<QuotedFusionEvidence> {
    let provenance = "plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md (implementation fb11a74f; u256 K=32; best fusion families weighted-union-0.25/mean and RRF-60/mean)".to_string();
    vec![
        QuotedFusionEvidence {
            tool: "glob".into(),
            bm25_rank: 256,
            semantic_rank: 256,
            fused_rank: 256,
            fused_margin: -0.2537,
            cause: "dual-signal-miss".into(),
            provenance: provenance.clone(),
        },
        QuotedFusionEvidence {
            tool: "table_filter".into(),
            bm25_rank: 256,
            semantic_rank: 230,
            fused_rank: 230,
            fused_margin: -0.0470,
            cause: "dual-signal-miss".into(),
            provenance: provenance.clone(),
        },
        QuotedFusionEvidence {
            tool: "write".into(),
            bm25_rank: 256,
            semantic_rank: 256,
            fused_rank: 256,
            fused_margin: -0.1910,
            cause: "dual-signal-miss".into(),
            provenance: provenance.clone(),
        },
        QuotedFusionEvidence {
            tool: "lsp_rename".into(),
            bm25_rank: 256,
            semantic_rank: 256,
            fused_rank: 256,
            fused_margin: -0.1431,
            cause: "dual-signal-miss".into(),
            provenance,
        },
    ]
}

/// Run the M001 persistent-miss audit on the expanded dev fixture.
///
/// For every missed relevant occurrence of the four tools at K=32 in the
/// expanded-u256 live BM25 ordering, records the full occurrence row.
/// Also returns aggregate BM25 recovery counts per universe for the
/// reproduction tripwire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissAuditReport {
    pub dev_cases: usize,
    pub eligible_relevant: BTreeMap<String, usize>,
    pub bm25_recovered: BTreeMap<String, usize>,
    pub occurrences: Vec<MissOccurrence>,
}

pub fn run_miss_audit() -> Result<MissAuditReport> {
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
    let mut eligible_relevant = BTreeMap::new();
    let mut bm25_recovered = BTreeMap::new();
    let mut occurrences = Vec::new();
    let quoted: BTreeMap<String, QuotedFusionEvidence> = quoted_m003_evidence()
        .into_iter()
        .map(|row| (row.tool.clone(), row))
        .collect();
    for universe in PREREG_UNIVERSES {
        let expanded = expand_universe_m001(&dev_cases, universe)?;
        let mut eligible = 0usize;
        let mut recovered = 0usize;
        for case in &expanded {
            let order = bm25_full_order(case);
            let rank_of: BTreeMap<&String, usize> = order
                .iter()
                .enumerate()
                .map(|(index, (name, _))| (name, index + 1))
                .collect();
            let in_top_k = |name: &String| rank_of.get(name).is_some_and(|rank| *rank <= 32);
            for (name, grade) in &case.relevance {
                eligible += 1;
                if in_top_k(name) {
                    recovered += 1;
                } else if universe == 256 && PERSISTENT_MISS_TOOLS.contains(&name.as_str()) {
                    occurrences.push(audit_occurrence(case, name, *grade, &order, &quoted)?);
                }
            }
        }
        eligible_relevant.insert(universe.to_string(), eligible);
        bm25_recovered.insert(universe.to_string(), recovered);
    }
    occurrences.sort_by(|left: &MissOccurrence, right: &MissOccurrence| {
        left.tool
            .cmp(&right.tool)
            .then_with(|| left.case_id.cmp(&right.case_id))
    });
    Ok(MissAuditReport {
        dev_cases: dev_cases.len(),
        eligible_relevant,
        bm25_recovered,
        occurrences,
    })
}

fn audit_occurrence(
    case: &ToolAdvisorCase,
    tool: &str,
    grade: u8,
    order: &[(String, f64)],
    quoted: &BTreeMap<String, QuotedFusionEvidence>,
) -> Result<MissOccurrence> {
    let candidate = case
        .candidates
        .iter()
        .find(|candidate| candidate.name == tool)
        .ok_or_else(|| anyhow!("expanded fixture lost relevant candidate {tool}"))?;
    let rank_of: BTreeMap<&String, usize> = order
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (name, index + 1))
        .collect();
    let bm25_rank = rank_of
        .get(&tool.to_string())
        .copied()
        .unwrap_or(usize::MAX);
    // Raw-score attribution via the catalog-formula mirror over the same
    // v1 documents the live ranker scored.
    let documents: Vec<String> = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| format!("{} {}", candidate.name, candidate.description))
        .collect();
    let (idf, avg_dl) = mirrored_idf(&documents);
    let bm25_score = mirrored_bm25_score(
        &case.context,
        &format!("{} {}", candidate.name, candidate.description),
        avg_dl,
        &idf,
    );
    let max_grade = case.relevance.values().copied().max().unwrap_or(grade);
    let preferred_order_position = case.preferred_order.iter().position(|name| name == tool);
    let schema_fields = live_builtin_schema(tool)
        .map(|schema| extract_schema_fields(&schema))
        .unwrap_or_default();
    let schema_cue_tokens: Vec<String> = schema_fields
        .iter()
        .flat_map(|field| {
            split_identifier_tokens(&field.name)
                .into_iter()
                .chain(signal_tokens(&field.description))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let query = AdvisorContextV2::from_benchmark_context(&case.context);
    let (mut class, mut rationale, support) = classify_inferability(
        &case.context,
        &candidate.name,
        &candidate.description,
        &schema_cue_tokens,
        grade,
        grade >= max_grade,
    );
    // Sibling-aware secondary adjudication: when the target shares the
    // highest grade (not uniquely primary) and carries no cue, but a
    // co-relevant sibling fully explains the query, the label represents
    // a later/supporting workflow step (implicit-secondary), not a
    // fixture mismatch. A uniquely-highest-grade label with no cue and
    // no explaining sibling stays other-evidence-defect.
    if class == InferabilityClass::OtherEvidenceDefect {
        let sibling_count = case
            .relevance
            .iter()
            .filter(|(name, sibling_grade)| name.as_str() != tool && **sibling_grade >= grade)
            .count();
        if sibling_count > 0 {
            let mut explaining: Option<(String, String)> = None;
            for (name, _) in case
                .relevance
                .iter()
                .filter(|(name, _)| name.as_str() != tool)
            {
                if let Some(sibling) = case
                    .candidates
                    .iter()
                    .find(|candidate| &candidate.name == name)
                {
                    if let Some(support) =
                        query_explicit_support(&case.context, &sibling.name, &sibling.description)
                    {
                        explaining = Some((sibling.name.clone(), support));
                        break;
                    }
                }
            }
            if let Some((sibling_name, sibling_support)) = explaining {
                class = InferabilityClass::ImplicitSecondary;
                rationale = format!(
                    "grade {grade} shares the case-highest grade with co-relevant '{sibling_name}' (query-explicit via '{sibling_support}'); no descriptor, schema, or paraphrase cue for '{tool}' is expressed, so the label represents a later/supporting workflow step not inferable from allowed query state"
                );
            }
        }
    }
    let nearest_wrong: Vec<String> = order.iter().take(5).map(|(name, _)| name.clone()).collect();
    let quoted_row = quoted.get(tool);
    Ok(MissOccurrence {
        tool: tool.to_string(),
        case_id: case.case_id.clone(),
        partition: "dev".into(),
        relevance_grade: grade,
        preferred_order_position: preferred_order_position,
        is_primary_highest_grade: grade >= max_grade,
        serialized_query: query.serialize(),
        candidate_name: candidate.name.clone(),
        candidate_description: candidate.description.clone(),
        candidate_category: candidate.category.clone(),
        candidate_disclosure: candidate.disclosure.clone(),
        live_schema_fields: schema_fields,
        bm25_rank,
        bm25_score_model: "mirrored-catalog-bm25-v1".into(),
        bm25_score,
        semantic_rank_quoted: quoted_row.map(|row| row.semantic_rank),
        fused_rank_quoted: quoted_row.map(|row| row.fused_rank),
        fused_margin_quoted: quoted_row.map(|row| row.fused_margin),
        lexical_overlap_terms: lexical_overlap_terms(case, candidate),
        nearest_wrong_candidates: nearest_wrong,
        inferability: class.as_str().into(),
        inferability_rationale: rationale,
        inferability_support: support,
    })
}

// ---- preregistration ----

/// Frozen M001 preregistration: miss audit summary, representation
/// schema/version, descriptor/query field contract, schema byte caps,
/// deterministic variant grid, conditional learned grid, dataset/dev
/// fingerprints, frozen gates, and the stop-condition result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M001Preregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub repository_baseline: String,
    pub dataset: String,
    pub dataset_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub fixture_fingerprint: String,
    pub expansion_code_version: u16,
    pub gates: BTreeMap<String, f64>,
    pub universes: Vec<usize>,
    pub primary_ks: Vec<usize>,
    pub query_field_order: Vec<String>,
    pub query_field_byte_cap: usize,
    pub query_total_byte_cap: usize,
    pub descriptor_field_order: Vec<String>,
    pub descriptor_schema_byte_cap: usize,
    pub descriptor_schema_field_desc_cap: usize,
    pub generic_normalization: Vec<String>,
    pub synonym_lexicon_entries: usize,
    pub deterministic_lexical_variants: Vec<String>,
    pub deterministic_semantic_variants: Vec<String>,
    pub deterministic_pooling_variants: Vec<String>,
    pub conditional_projection_families: Vec<String>,
    pub conditional_lr_grid: Vec<f64>,
    pub conditional_max_epochs: u32,
    pub conditional_temperature_grid: Vec<f64>,
    pub conditional_loss_weights: BTreeMap<String, f64>,
    pub conditional_seeds: Vec<u64>,
    pub conditional_hard_negatives_per_positive: usize,
    pub conditional_batch_size: usize,
    pub conditional_max_params: u64,
    pub schema_plumbing: SchemaPlumbingAudit,
    pub quoted_fusion_evidence: Vec<QuotedFusionEvidence>,
    pub miss_audit: MissAuditReport,
    pub stop_condition: String,
    pub m002_ready: bool,
}

pub fn m001_preregistration(repository_baseline: &str) -> Result<M001Preregistration> {
    let cases = load_cases(None).context("load builtin corpus")?;
    let dataset_fingerprint_value = dataset_fingerprint(&cases).context("fingerprint dataset")?;
    let partition = partition_cases(&cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases).context("fingerprint dev partition")?;
    let receipt_anchor = serde_json::to_vec(&serde_json::json!({
        "protocol": RETRIEVAL_SIGNAL_M001_PROTOCOL,
        "schema": RETRIEVAL_SIGNAL_SCHEMA_VERSION,
    }))
    .context("serialize receipt anchor")?;
    let mut hasher = Sha256::new();
    hasher.update(b"retrieval-signal-m001-fixtures-v1\n");
    hasher.update(&receipt_anchor);
    hasher.update(b"\n");
    hasher.update(dev_fp.as_bytes());
    hasher.update(b"\n");
    hasher.update(M001_EXPANSION_CODE_VERSION.to_le_bytes());
    let fixture_fp = hex::encode(hasher.finalize());
    let miss_audit = run_miss_audit()?;
    // Hard stop: any gate-critical relevant label classified as a hidden
    // workflow dependency or fixture defect blocks M002 and requires a
    // separate retrieval-evaluation corrective.
    let blocking = miss_audit
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.inferability == "implicit-secondary"
                || occurrence.inferability == "other-evidence-defect"
        })
        .collect::<Vec<_>>();
    let (stop_condition, m002_ready) = if blocking.is_empty() {
        (
            "pass: every audited gate-critical label is inferable from allowed AdvisorContextV2 state; M002 deterministic Signal V2 is authorized".to_string(),
            true,
        )
    } else {
        (
            format!(
                "HARD STOP: {} occurrence(s) classified as evaluation defects ({}); M002 stays blocked; register a separate retrieval-evaluation corrective",
                blocking.len(),
                blocking
                    .iter()
                    .map(|occurrence| format!(
                        "{}@{}:{}",
                        occurrence.tool, occurrence.case_id, occurrence.inferability
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            false,
        )
    };
    let mut gates = BTreeMap::new();
    gates.insert("recall_64".to_string(), GATE_RECALL_64);
    gates.insert("recall_128".to_string(), GATE_RECALL_128);
    gates.insert("recall_256".to_string(), GATE_RECALL_256);
    let mut loss_weights = BTreeMap::new();
    loss_weights.insert("contrastive".to_string(), 1.0);
    loss_weights.insert("graded_positive".to_string(), 1.0);
    loss_weights.insert("hard_negative".to_string(), 0.5);
    loss_weights.insert("same_family_margin".to_string(), 0.1);
    Ok(M001Preregistration {
        schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
        protocol: RETRIEVAL_SIGNAL_M001_PROTOCOL.into(),
        repository_baseline: repository_baseline.into(),
        dataset: "assets/tool-advisor/corpus.jsonl".into(),
        dataset_fingerprint: dataset_fingerprint_value,
        dev_partition_fingerprint: dev_fp,
        fixture_fingerprint: fixture_fp,
        expansion_code_version: M001_EXPANSION_CODE_VERSION,
        gates,
        universes: PREREG_UNIVERSES.into(),
        primary_ks: PREREG_PRIMARY_KS.into(),
        query_field_order: QUERY_FIELD_ORDER.iter().map(ToString::to_string).collect(),
        query_field_byte_cap: QUERY_FIELD_BYTE_CAP,
        query_total_byte_cap: QUERY_TOTAL_BYTE_CAP,
        descriptor_field_order: DESCRIPTOR_FIELD_ORDER
            .iter()
            .map(ToString::to_string)
            .collect(),
        descriptor_schema_byte_cap: DESCRIPTOR_SCHEMA_BYTE_CAP,
        descriptor_schema_field_desc_cap: DESCRIPTOR_SCHEMA_FIELD_DESC_CAP,
        generic_normalization: vec![
            "identifier-decomposition".into(),
            "lowercase-unicode-normalization".into(),
            "punctuation-separation".into(),
            "conservative-shared-morphology".into(),
        ],
        synonym_lexicon_entries: PREREGISTERED_SYNONYM_LEXICON_ENTRIES,
        deterministic_lexical_variants: DETERMINISTIC_LEXICAL_VARIANTS
            .iter()
            .map(ToString::to_string)
            .collect(),
        deterministic_semantic_variants: DETERMINISTIC_SEMANTIC_VARIANTS
            .iter()
            .map(ToString::to_string)
            .collect(),
        deterministic_pooling_variants: DETERMINISTIC_POOLING_VARIANTS
            .iter()
            .map(ToString::to_string)
            .collect(),
        conditional_projection_families: PROJECTION_FAMILIES
            .iter()
            .map(ToString::to_string)
            .collect(),
        conditional_lr_grid: PROJECTION_LR_GRID.into(),
        conditional_max_epochs: PROJECTION_MAX_EPOCHS,
        conditional_temperature_grid: PROJECTION_TEMPERATURE_GRID.into(),
        conditional_loss_weights: loss_weights,
        conditional_seeds: PROJECTION_SEEDS.into(),
        conditional_hard_negatives_per_positive: PROJECTION_HARD_NEGATIVES_PER_POSITIVE,
        conditional_batch_size: PROJECTION_BATCH_SIZE,
        conditional_max_params: MAX_PROJECTION_PARAMS,
        schema_plumbing: schema_plumbing_audit(),
        quoted_fusion_evidence: quoted_m003_evidence(),
        miss_audit,
        stop_condition,
        m002_ready,
    })
}

/// Canonical preregistration hash. Any grid/field change alters the hash;
/// v3-derived inputs are rejected before hashing.
pub fn prereg_fingerprint(prereg: &M001Preregistration) -> Result<String> {
    let bytes = serde_json::to_vec(prereg).context("serialize preregistration")?;
    let text = String::from_utf8(bytes).context("preregistration is UTF-8")?;
    if text.contains("qualification-v3-holdout") || text.contains("sequence-qualification-v3") {
        return Err(anyhow!("preregistration must not take v3 selection inputs"));
    }
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(prereg).context("serialize preregistration for hash")?,
    )))
}

// ---- M006 retrieval-evaluation corrective ----
//
// M001 closed blocked on its §4 hard stop: three gate-critical occurrence
// rows carry `implicit-secondary` labels (supporting-workflow steps whose
// queries are fully explained by a co-relevant sibling tool). M006 owns the
// product/evaluation decision the experiment is forbidden from making
// implicitly: what retrieval relevance is *for*. It re-derives the
// gate-critical set mechanically over the frozen M001 rows without
// relabeling history, then verdicts M002.
//
// Decision (recorded with rationale; operator-supplied product judgment):
// - `AllWorkflow`: every plausible workflow tool counts, including
//   supporting steps. No row leaves the gate-critical set.
// - `CurrentStepOnly`: only tools inferable from the bounded current-step
//   query count. `implicit-secondary` rows leave the gate-critical set;
//   every exclusion cites its occurrence row + this rule.
// - `GradedRecall`: supporting steps count with reduced weight. All rows
//   stay in the set with explicit weights; uninferable rows still block
//   M002 because no graded weight was preregistered in M001.
//
// M002 is unblocked iff the re-derived included set contains zero
// `implicit-secondary` / `other-evidence-defect` rows. Gates themselves are
// never lowered: re-derived denominators and required-hit counts are
// recomputed from the decision and reported alongside the verdict.

/// Intended retrieval relevance target. Variants are exhaustive; the
/// decision is recorded explicitly in the M006 receipt, never inferred
/// from recall numbers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RelevanceTarget {
    AllWorkflow,
    CurrentStepOnly,
    GradedRecall,
}

impl RelevanceTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllWorkflow => "all-workflow",
            Self::CurrentStepOnly => "current-step-only",
            Self::GradedRecall => "graded-recall",
        }
    }

    /// Human-readable rule applied by [`gate_critical_set`] for this target.
    pub fn exclusion_rule(self) -> &'static str {
        match self {
            Self::AllWorkflow => {
                "all-workflow: no occurrence leaves the gate-critical set; supporting steps count fully"
            }
            Self::CurrentStepOnly => {
                "current-step-only: implicit-secondary rows leave the gate-critical set (supporting-workflow steps are not current-step needs); all other rows stay"
            }
            Self::GradedRecall => {
                "graded-recall: all rows stay with reduced weight for implicit-secondary (0.5 vs 1.0); uninferable rows still block M002 absent preregistered weights"
            }
        }
    }
}

/// Recorded M006 relevance-target decision. The rationale is required and
/// must be non-empty: a decision without recorded rationale is invalid
/// output per the plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelevanceDecision {
    pub target: RelevanceTarget,
    pub rationale: String,
    pub repository_baseline: String,
}

/// Decide the relevance target with an explicit product rationale.
///
/// Rejects an empty/whitespace rationale. Does not touch frozen corpora,
/// thresholds, K, or gates; it only records the judgment M002 needs.
pub fn decide_relevance_target(
    target: RelevanceTarget,
    rationale: &str,
    repository_baseline: &str,
) -> Result<RelevanceDecision> {
    if rationale.trim().is_empty() {
        return Err(anyhow!(
            "relevance-target decision requires a non-empty rationale"
        ));
    }
    Ok(RelevanceDecision {
        target,
        rationale: rationale.trim().to_string(),
        repository_baseline: repository_baseline.to_string(),
    })
}

/// One row excluded from the gate-critical set under the decision, with
/// the occurrence identity plus the rule citation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateCriticalExclusion {
    pub tool: String,
    pub case_id: String,
    pub inferability: String,
    pub rule: String,
}

/// Split frozen M001 occurrence rows into gate-critical included rows plus
/// cited exclusions, per the decided target. No corpus bytes are read or
/// rewritten; the input rows are the frozen audit output.
pub fn gate_critical_set(
    target: RelevanceTarget,
    occurrences: &[MissOccurrence],
) -> (Vec<MissOccurrence>, Vec<GateCriticalExclusion>) {
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    for occurrence in occurrences {
        let is_secondary = occurrence.inferability == "implicit-secondary";
        match target {
            RelevanceTarget::AllWorkflow => included.push(occurrence.clone()),
            RelevanceTarget::CurrentStepOnly => {
                if is_secondary {
                    excluded.push(GateCriticalExclusion {
                        tool: occurrence.tool.clone(),
                        case_id: occurrence.case_id.clone(),
                        inferability: occurrence.inferability.clone(),
                        rule: target.exclusion_rule().to_string(),
                    });
                } else {
                    included.push(occurrence.clone());
                }
            }
            RelevanceTarget::GradedRecall => included.push(occurrence.clone()),
        }
    }
    (included, excluded)
}

/// Weight of one occurrence under the decided target. Only `GradedRecall`
/// differentiates; the weights are explicit here because M001 preregistered
/// no graded weights, so a graded verdict still blocks M002 (see
/// [`m002_unblocked`]).
pub fn occurrence_weight(target: RelevanceTarget, occurrence: &MissOccurrence) -> f64 {
    match target {
        RelevanceTarget::GradedRecall if occurrence.inferability == "implicit-secondary" => 0.5,
        _ => 1.0,
    }
}

/// Re-derive per-universe eligible denominators after exclusions. Each
/// excluded occurrence row belongs to one dev case present in every
/// expanded universe, so every universe denominator drops by the number of
/// distinct excluded `(tool, case_id)` pairs. Returns the re-derived map
/// plus the distinct exclusion count for receipt transparency.
pub fn rederived_eligible_counts(
    eligible_relevant: &BTreeMap<String, usize>,
    excluded: &[GateCriticalExclusion],
) -> (BTreeMap<String, usize>, usize) {
    let mut distinct = BTreeSet::new();
    for row in excluded {
        distinct.insert((row.tool.clone(), row.case_id.clone()));
    }
    let dropped = distinct.len();
    let mut rederived = BTreeMap::new();
    for (universe, eligible) in eligible_relevant {
        rederived.insert(universe.clone(), eligible.saturating_sub(dropped));
    }
    (rederived, dropped)
}

/// Required hits for a recall gate at `ceil(denominator * gate)`.
/// Gates themselves are never lowered by the decision.
pub fn required_hits(denominator: usize, gate: f64) -> usize {
    (denominator as f64 * gate).ceil() as usize
}

/// M002 unblock verdict over the re-derived included set: unblocked iff no
/// included row carries an evaluation-defect class. Under `GradedRecall`
/// the verdict stays blocked because reduced weight does not make an
/// uninferable label inferable absent preregistered weights.
pub fn m002_unblocked(target: RelevanceTarget, included: &[MissOccurrence]) -> bool {
    let has_defect = included.iter().any(|occurrence| {
        occurrence.inferability == "implicit-secondary"
            || occurrence.inferability == "other-evidence-defect"
    });
    match target {
        RelevanceTarget::CurrentStepOnly => !has_defect,
        RelevanceTarget::AllWorkflow | RelevanceTarget::GradedRecall => !has_defect,
    }
}

/// Machine-readable M006 decision receipt: decision log, re-derived
/// denominators, required-hit counts under the frozen gates, re-audit
/// classifications restricted to the derived set, and the explicit M002
/// verdict. Readers must tolerate absence of these fields on the older
/// M001 receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M006DecisionReceipt {
    pub schema_version: u16,
    pub protocol: String,
    pub repository_baseline: String,
    pub dataset: String,
    pub dataset_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub decision: RelevanceDecision,
    pub exclusion_rule: String,
    pub included_occurrences: Vec<MissOccurrence>,
    pub excluded_occurrences: Vec<GateCriticalExclusion>,
    pub rederived_eligible_relevant: BTreeMap<String, usize>,
    pub required_hits: BTreeMap<String, usize>,
    pub occurrence_weights: BTreeMap<String, f64>,
    pub m002_unblocked: bool,
    pub verdict: String,
}

/// M006 protocol identity.
pub const M006_PROTOCOL: &str = "retrieval-signal-m006-decision-v1";
/// M006 receipt schema version.
pub const M006_SCHEMA_VERSION: u16 = 1;

/// Build the M006 decision receipt from frozen M001 inputs plus the
/// recorded decision. Re-runs no encoder and trains nothing; it filters
/// the frozen `run_miss_audit` rows and recomputes denominators.
pub fn m006_decision_receipt(decision: &RelevanceDecision) -> Result<M006DecisionReceipt> {
    let cases = load_cases(None).context("load builtin corpus")?;
    let dataset_fp = dataset_fingerprint(&cases).context("fingerprint dataset")?;
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
    let audit = run_miss_audit()?;
    let (included, excluded) = gate_critical_set(decision.target, &audit.occurrences);
    let (rederived, _) = rederived_eligible_counts(&audit.eligible_relevant, &excluded);
    let mut required = BTreeMap::new();
    for (universe, denominator) in &rederived {
        let gate = match universe.as_str() {
            "64" => GATE_RECALL_64,
            "128" => GATE_RECALL_128,
            "256" => GATE_RECALL_256,
            _ => continue,
        };
        required.insert(
            format!("required_hits_{universe}"),
            required_hits(*denominator, gate),
        );
    }
    let mut weights = BTreeMap::new();
    for occurrence in &included {
        weights.insert(
            format!("{}@{}", occurrence.tool, occurrence.case_id),
            occurrence_weight(decision.target, occurrence),
        );
    }
    let unblocked = m002_unblocked(decision.target, &included);
    let verdict = if unblocked {
        format!(
            "M002 unblocked: re-derived gate-critical set under {} is fully inferable ({} included, {} excluded); deterministic Signal V2 sweep authorized on the re-derived denominators",
            decision.target.as_str(),
            included.len(),
            excluded.len()
        )
    } else {
        format!(
            "workstream negative close owned here: re-derived gate-critical set under {} still contains evaluation-defect rows ({} included with defects); M002 stays blocked; gates unchanged",
            decision.target.as_str(),
            included.len()
        )
    };
    // v3 must never inform the decision: the receipt carries no v3 input
    // and the builder never loads v3 fixtures.
    Ok(M006DecisionReceipt {
        schema_version: M006_SCHEMA_VERSION,
        protocol: M006_PROTOCOL.into(),
        repository_baseline: decision.repository_baseline.clone(),
        dataset: "assets/tool-advisor/corpus.jsonl".into(),
        dataset_fingerprint: dataset_fp,
        dev_partition_fingerprint: dev_fp,
        decision: decision.clone(),
        exclusion_rule: decision.target.exclusion_rule().to_string(),
        included_occurrences: included,
        excluded_occurrences: excluded,
        rederived_eligible_relevant: rederived,
        required_hits: required,
        occurrence_weights: weights,
        m002_unblocked: unblocked,
        verdict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_splitting_is_deterministic() {
        assert_eq!(
            split_identifier_tokens("table_filter"),
            vec!["table", "filter"]
        );
        assert_eq!(split_identifier_tokens("lsp_rename"), vec!["lsp", "rename"]);
        assert_eq!(split_identifier_tokens("glob"), vec!["glob"]);
        assert_eq!(
            split_identifier_tokens("camelCase-kebab_mixed.name"),
            vec!["camel", "case", "kebab", "mixed", "name"]
        );
        assert_eq!(
            split_identifier_tokens("table_filter"),
            split_identifier_tokens("table_filter")
        );
    }

    #[test]
    fn schema_extraction_excludes_value_bearing_keywords() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file",
                    "examples": ["secret/path"],
                    "default": "fallback"
                },
                "token": {
                    "type": "string",
                    "description": "Credential token",
                    "const": "hardcoded",
                    "enum": ["a", "b"]
                }
            },
            "required": ["path"]
        });
        let fields = extract_schema_fields(&schema);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "path");
        assert!(fields[0].required);
        assert!(!fields[1].required);
        let serialized = serde_json::to_string(&fields).expect("serialize fields");
        assert!(!serialized.contains("secret/path"));
        assert!(!serialized.contains("fallback"));
        assert!(!serialized.contains("hardcoded"));
        assert!(!serialized.contains("\"enum\""));
        assert!(!serialized.contains("examples"));
    }

    #[test]
    fn schema_extraction_orders_fields_and_caps_bytes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "zeta": {"type": "string", "description": "z"},
                "alpha": {"type": "string", "description": "a"}
            }
        });
        let fields = extract_schema_fields(&schema);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert!(schema_fields_bytes(&fields) <= DESCRIPTOR_SCHEMA_BYTE_CAP);
    }

    #[test]
    fn descriptor_field_order_is_stable() {
        let candidate = ToolAdvisorCandidate {
            name: "glob".into(),
            description: "Find files by path pattern".into(),
            category: "ReadOnly".into(),
            disclosure: "deferred".into(),
            synthetic_identity: false,
        };
        let descriptor = RetrievalDescriptorV2::from_candidate(
            &candidate,
            extract_schema_fields(&live_builtin_schema("glob").expect("glob schema")),
        );
        let flat = descriptor.flat_text();
        let labelled = descriptor.field_labelled_text();
        for text in [&flat, &labelled] {
            let name_pos = text.find("glob").expect("name present");
            let desc_pos = text.find("Find files").expect("description present");
            assert!(name_pos < desc_pos);
        }
        assert!(labelled.contains("schema: "));
        assert!(labelled.contains("pattern:string"));
        assert_eq!(descriptor.identifier_tokens, vec!["glob"]);
    }

    #[test]
    fn candidate_without_schema_remains_valid() {
        let candidate = ToolAdvisorCandidate {
            name: "table_filter".into(),
            description: "Filter tabular data".into(),
            category: "ReadOnly".into(),
            disclosure: "deferred".into(),
            synthetic_identity: false,
        };
        assert!(live_builtin_schema("table_filter").is_none());
        let descriptor = RetrievalDescriptorV2::from_candidate(&candidate, Vec::new());
        assert!(descriptor.schema_fields.is_empty());
        assert!(descriptor.flat_text().contains("table_filter"));
        assert!(extract_schema_fields(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn query_serializer_excludes_transcript_results_and_secrets() {
        let context = AdvisorContextV2::from_benchmark_context(
            "Locate every test fixture; api_key=do-not-leak",
        );
        let query = RetrievalQueryV2::from_advisor_context(&context);
        let serialized = query.serialize();
        assert!(serialized.starts_with("retrieval-signal-v2\n"));
        assert!(!serialized.contains("do-not-leak"));
        assert!(!serialized.contains("transcript"));
        assert!(!serialized.contains("tool result"));
        for field in QUERY_FIELD_ORDER {
            if serialized.contains(field) {
                assert!(serialized.contains(&format!("{field}:")));
            }
        }
        assert!(serialized.len() <= QUERY_TOTAL_BYTE_CAP);
    }

    #[test]
    fn miss_audit_reproduces_four_persistent_misses() {
        let report = run_miss_audit().expect("miss audit runs");
        assert_eq!(report.dev_cases, M004_DEV_CASES);
        assert_eq!(
            report.eligible_relevant.get("256").copied().unwrap_or(0),
            M004_ELIGIBLE_RELEVANT
        );
        assert_eq!(
            report.bm25_recovered.get("256").copied().unwrap_or(0),
            M004_BM25_RECOVERED
        );
        for tool in PERSISTENT_MISS_TOOLS {
            assert!(
                report
                    .occurrences
                    .iter()
                    .any(|occurrence| occurrence.tool == tool),
                "expected at least one audited miss for {tool}"
            );
        }
    }

    #[test]
    fn inferability_classification_always_carries_supporting_text() {
        let report = run_miss_audit().expect("miss audit runs");
        assert!(!report.occurrences.is_empty());
        for occurrence in &report.occurrences {
            assert!(
                !occurrence.inferability_rationale.is_empty(),
                "missing rationale for {}@{}",
                occurrence.tool,
                occurrence.case_id
            );
            assert!(
                !occurrence.inferability_support.is_empty(),
                "missing supporting text for {}@{}",
                occurrence.tool,
                occurrence.case_id
            );
            assert!(
                occurrence.serialized_query.contains(
                    &occurrence.inferability_support
                        [..occurrence.inferability_support.len().min(24)]
                ),
                "supporting text must come from allowed query state for {}@{}",
                occurrence.tool,
                occurrence.case_id
            );
        }
    }

    #[test]
    fn preregistration_hash_changes_on_grid_or_field_change() {
        let prereg = m001_preregistration("test-baseline").expect("preregistration builds");
        let hash = prereg_fingerprint(&prereg).expect("hash builds");
        let mut mutated = prereg.clone();
        mutated
            .deterministic_lexical_variants
            .push("post-hoc-variant".into());
        assert_ne!(hash, prereg_fingerprint(&mutated).expect("mutated hash"));
        let mut reordered = prereg.clone();
        reordered.query_field_order.swap(0, 1);
        assert_ne!(
            hash,
            prereg_fingerprint(&reordered).expect("reordered hash")
        );
    }

    #[test]
    fn v3_is_absent_from_selection_inputs() {
        let prereg = m001_preregistration("test-baseline").expect("preregistration builds");
        assert_eq!(prereg.dataset, "assets/tool-advisor/corpus.jsonl");
        let serialized = serde_json::to_string(&prereg).expect("serialize prereg");
        assert!(!serialized.contains("qualification-v3-holdout"));
        assert!(!serialized.contains("v3-holdout"));
        assert!(prereg_fingerprint(&prereg).is_ok());
    }

    #[test]
    fn descriptor_cache_key_contains_no_user_context() {
        let candidate = ToolAdvisorCandidate {
            name: "write".into(),
            description: "Write content to files".into(),
            category: "Edit".into(),
            disclosure: "deferred".into(),
            synthetic_identity: false,
        };
        let descriptor = RetrievalDescriptorV2::from_candidate(
            &candidate,
            extract_schema_fields(&live_builtin_schema("write").expect("write schema")),
        );
        let key = RetrievalCacheKeyV2 {
            representation_schema_version: RETRIEVAL_SIGNAL_SCHEMA_VERSION,
            descriptor_fingerprint: descriptor_fingerprint_v2(&descriptor),
            encoder_tokenizer_version: "test-tokenizer".into(),
            pooling: "mean".into(),
            surface_fingerprint: "test-surface".into(),
        };
        let serialized = serde_json::to_string(&key).expect("serialize key");
        assert!(!serialized.contains("Locate every"));
        assert_eq!(key.representation_schema_version, 2);
    }

    #[test]
    fn m006_decision_variants_are_exhaustive_and_round_trip() {
        for target in [
            RelevanceTarget::AllWorkflow,
            RelevanceTarget::CurrentStepOnly,
            RelevanceTarget::GradedRecall,
        ] {
            let decision = decide_relevance_target(target, "test rationale", "test-baseline")
                .expect("decision builds");
            assert_eq!(decision.target, target);
            assert!(!decision.rationale.is_empty());
            assert!(!decision.target.exclusion_rule().is_empty());
            let serialized = serde_json::to_string(&decision).expect("serialize decision");
            let round_trip: RelevanceDecision =
                serde_json::from_str(&serialized).expect("round trip");
            assert_eq!(round_trip, decision);
        }
        assert!(decide_relevance_target(RelevanceTarget::CurrentStepOnly, "   ", "x").is_err());
        assert!(decide_relevance_target(RelevanceTarget::CurrentStepOnly, "", "x").is_err());
    }

    #[test]
    fn m006_current_step_only_excludes_exactly_the_three_secondary_rows() {
        let audit = run_miss_audit().expect("miss audit runs");
        assert_eq!(audit.occurrences.len(), 4);
        let (included, excluded) =
            gate_critical_set(RelevanceTarget::CurrentStepOnly, &audit.occurrences);
        assert_eq!(excluded.len(), 3);
        assert_eq!(included.len(), 1);
        for row in &excluded {
            assert_eq!(row.inferability, "implicit-secondary");
            assert!(row.rule.contains("current-step-only"));
        }
        // No inferable row is excluded: the surviving row is the
        // query-paraphrase glob occurrence.
        assert_eq!(included[0].tool, "glob");
        assert_eq!(included[0].inferability, "query-paraphrase");
        // AllWorkflow keeps everything; GradedRecall keeps everything.
        let (all_included, all_excluded) =
            gate_critical_set(RelevanceTarget::AllWorkflow, &audit.occurrences);
        assert_eq!(all_included.len(), 4);
        assert!(all_excluded.is_empty());
        let (graded_included, graded_excluded) =
            gate_critical_set(RelevanceTarget::GradedRecall, &audit.occurrences);
        assert_eq!(graded_included.len(), 4);
        assert!(graded_excluded.is_empty());
    }

    #[test]
    fn m006_rederived_denominators_and_required_hits_follow_frozen_gates() {
        let audit = run_miss_audit().expect("miss audit runs");
        let (_, excluded) = gate_critical_set(RelevanceTarget::CurrentStepOnly, &audit.occurrences);
        let (rederived, dropped) = rederived_eligible_counts(&audit.eligible_relevant, &excluded);
        assert_eq!(dropped, 3);
        assert_eq!(rederived.get("256").copied().unwrap_or(0), 69);
        assert_eq!(rederived.get("128").copied().unwrap_or(0), 69);
        assert_eq!(rederived.get("64").copied().unwrap_or(0), 69);
        assert_eq!(required_hits(69, GATE_RECALL_256), 66);
        assert_eq!(required_hits(69, GATE_RECALL_128), 68);
        assert_eq!(required_hits(69, GATE_RECALL_64), 69);
        // Frozen corpus unchanged: dev fingerprint still matches.
        let cases = load_cases(None).expect("load corpus");
        let partition = partition_cases(&cases);
        let dev_cases: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        assert_eq!(
            dataset_fingerprint(&dev_cases).expect("fingerprint dev"),
            EXPECTED_DEV_PARTITION_FINGERPRINT
        );
    }

    #[test]
    fn m006_receipt_carries_no_v3_input() {
        let decision = decide_relevance_target(
            RelevanceTarget::CurrentStepOnly,
            "test rationale: supporting steps are not current-step needs",
            "test-baseline",
        )
        .expect("decision builds");
        let receipt = m006_decision_receipt(&decision).expect("receipt builds");
        assert_eq!(receipt.protocol, M006_PROTOCOL);
        assert_eq!(receipt.schema_version, M006_SCHEMA_VERSION);
        let serialized = serde_json::to_string(&receipt).expect("serialize receipt");
        assert!(!serialized.contains("qualification-v3-holdout"));
        assert!(!serialized.contains("sequence-qualification-v3"));
        assert!(!serialized.contains("v3-holdout"));
    }

    #[test]
    fn m006_current_step_only_unblocks_m002_while_all_workflow_stays_blocked() {
        let audit = run_miss_audit().expect("miss audit runs");
        let (current_included, _) =
            gate_critical_set(RelevanceTarget::CurrentStepOnly, &audit.occurrences);
        assert!(m002_unblocked(
            RelevanceTarget::CurrentStepOnly,
            &current_included
        ));
        let (all_included, _) = gate_critical_set(RelevanceTarget::AllWorkflow, &audit.occurrences);
        assert!(!m002_unblocked(RelevanceTarget::AllWorkflow, &all_included));
        let (graded_included, _) =
            gate_critical_set(RelevanceTarget::GradedRecall, &audit.occurrences);
        assert!(!m002_unblocked(
            RelevanceTarget::GradedRecall,
            &graded_included
        ));
    }
}
