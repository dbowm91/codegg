//! M001R signal preregistration after retrieval-evaluation corrective.
//!
//! M001 reached its planned evaluation-target hard stop without freezing
//! Signal V2 or learned-projection degrees of freedom. C001 closed with
//! disposition B (real inferable gap, no ranker blocker). This module freezes
//! the corrected retrieval target binding plus every Signal V2 and
//! conditional-projection degree of freedom M002/M003 may consume.
//!
//! Scope boundary: preregistration constants and validation only. No retriever
//! variant is implemented here, no gate is relaxed, no per-tool alias is
//! added, and v3/future-v4 never select fields or hyperparameters.

use super::retrieval_relevance::{
    build_derived_view, derived_view_fingerprint, impact_by_split,
    EXPECTED_CORPUS_FINGERPRINT as RELEVANCE_CORPUS_FP,
    EXPECTED_DEV_PARTITION_FINGERPRINT as RELEVANCE_DEV_FP,
};
use super::{dataset_fingerprint, partition_cases};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Schema version for [`SignalPreregistration`].
pub const SIGNAL_PREREG_SCHEMA_VERSION: u16 = 1;
/// Preregistration protocol name bound into the receipt and M002/M003 configs.
pub const SIGNAL_PREREG_PROTOCOL: &str = "m001r-preregistered-signal-v2-v1";
/// Repository-owned receipt path (relative to workspace root).
pub const SIGNAL_PREREG_ASSET_PATH: &str =
    "assets/tool-advisor/retrieval-signal-m001r-preregistration.json";
/// C001 closure record that authorizes this preregistration.
pub const C001_CLOSURE_PATH: &str =
    "plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md";
/// C001 disposition. M001R exists only for disposition B.
pub const C001_DISPOSITION: &str = "B";
/// C001 implementation commit reviewed for this preregistration.
pub const C001_IMPLEMENTATION_COMMIT: &str = "8613575d";
/// Frozen historical corpus path.
pub const PREREG_DATASET: &str = "assets/tool-advisor/corpus.jsonl";
/// Frozen derived retrieval-relevance view path from C001.
pub const PREREG_DERIVED_VIEW: &str = "assets/tool-advisor/retrieval-relevance-v1.json";
/// Adjudication version bound from C001.
pub const PREREG_ADJUDICATION_VERSION: &str = "retrieval-relevance-v1";

/// Corrected target binding: frozen corpus fingerprint (C001 §3).
pub const EXPECTED_CORPUS_FINGERPRINT: &str =
    "06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582";
/// Corrected target binding: frozen dev-partition fingerprint.
pub const EXPECTED_DEV_PARTITION_FINGERPRINT: &str =
    "b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9";
/// Corrected target binding: derived-view fingerprint (280 entries).
pub const EXPECTED_DERIVED_VIEW_FINGERPRINT: &str =
    "23edef17d0e4f4ac71d4e4b65349585cfc9b3c08d49cd4f7ac47e3bad12b5479";

/// Corrected eligible counts per split (current+explicit / implicit).
/// From C001 §3: train 113/0/25, dev 53/0/19, test 53/0/17.
pub const EXPECTED_TRAIN_CURRENT: usize = 113;
pub const EXPECTED_TRAIN_EXPLICIT: usize = 0;
pub const EXPECTED_TRAIN_IMPLICIT: usize = 25;
pub const EXPECTED_DEV_CURRENT: usize = 53;
pub const EXPECTED_DEV_EXPLICIT: usize = 0;
pub const EXPECTED_DEV_IMPLICIT: usize = 19;
pub const EXPECTED_TEST_CURRENT: usize = 53;
pub const EXPECTED_TEST_EXPLICIT: usize = 0;
pub const EXPECTED_TEST_IMPLICIT: usize = 17;

/// Retrieval gates, unchanged from M004/R001 (64>=0.99, 128>=0.98, 256>=0.95).
pub const GATE_RECALL_64: f64 = 0.99;
pub const GATE_RECALL_128: f64 = 0.98;
pub const GATE_RECALL_256: f64 = 0.95;
/// Preregistered candidate universes (deferred tools per case).
pub const PREREG_UNIVERSES: [usize; 3] = [64, 128, 256];
/// Primary shortlists. Gates must clear at K<=32 first.
pub const PREREG_PRIMARY_KS: [usize; 3] = [16, 24, 32];

/// Frozen MiniLM encoder manifest M002/M003 must reuse (reference asset M004
/// used). The transformer stays frozen; only the small projection heads train.
pub const PREREG_ENCODER_MANIFEST: &str =
    "target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json";
/// Frozen downstream ranker selection (span-packed, M003). Retrieval work must
/// not retrain or re-tune it.
pub const PREREG_RANKER_SELECTION: &str =
    "assets/tool-advisor/order-invariance-m003-selection.json";
/// MiniLM query/descriptor embedding width.
pub const MINILM_DIM: usize = 384;
/// Retrieval projection output width.
pub const PROJECTION_DIM: usize = 128;
/// Hard cap on trainable retrieval-projection parameters.
pub const PROJECTION_MAX_PARAMS: usize = 500_000;

// ---- frozen Signal V2 representation contract ----

/// Candidate descriptor fields, in canonical construction order.
pub const CANDIDATE_FIELDS: [&str; 8] = [
    "canonical_name",
    "identifier_tokens",
    "description",
    "category",
    "disclosure",
    "schema_property_names",
    "schema_property_descriptions",
    "operation_terms",
];

/// Per-field byte caps for candidate descriptor construction.
pub const CANDIDATE_FIELD_CAPS: [(&str, usize); 8] = [
    ("canonical_name", 256),
    ("identifier_tokens", 256),
    ("description", 1024),
    ("category", 128),
    ("disclosure", 128),
    ("schema_property_names", 1024),
    ("schema_property_descriptions", 1024),
    ("operation_terms", 512),
];

/// Total descriptor byte cap (sum of capped fields, enforced before embed).
pub const DESCRIPTOR_TOTAL_CAP_BYTES: usize = 4096;

/// Query fields from `AdvisorContextV2`, in canonical construction order.
/// Frozen benchmark cases populate only `current_objective`; the remaining
/// fields are frozen for live/future use and stay empty offline.
pub const QUERY_FIELDS: [&str; 5] = [
    "current_objective",
    "current_task",
    "next_steps",
    "unresolved_signal",
    "capability_cue",
];

/// Per-field byte caps for query construction.
pub const QUERY_FIELD_CAPS: [(&str, usize); 5] = [
    ("current_objective", 2048),
    ("current_task", 2048),
    ("next_steps", 2048),
    ("unresolved_signal", 1024),
    ("capability_cue", 1024),
];

/// Total query byte cap, matching `MAX_ADVISOR_CONTEXT_V2_BYTES`.
pub const QUERY_TOTAL_CAP_BYTES: usize = 8 * 1024;

/// At most two next-step entries enter the query (matches `AdvisorContextV2`).
pub const QUERY_MAX_NEXT_STEPS: usize = 2;

/// Field-labelled framing tags for semantic descriptor/query variants.
/// Labels are generic field names, never per-tool text.
pub const FIELD_LABELS: [&str; 8] = [
    "name",
    "identifiers",
    "description",
    "category",
    "disclosure",
    "schema",
    "schema-descriptions",
    "operations",
];

/// Frozen lexical field weights for the `field-weighted-bm25-v2` variant.
/// Fixed by preregistration; M002 must not tune new weights after measurement.
pub const LEXICAL_FIELD_WEIGHTS: [(&str, f64); 7] = [
    ("canonical_name", 3.0),
    ("identifier_tokens", 2.0),
    ("operation_terms", 2.0),
    ("schema_property_names", 1.5),
    ("description", 1.0),
    ("category", 0.5),
    ("disclosure", 0.5),
];

/// Deterministic retrieval mode grid M002 must measure (no additions).
pub const DETERMINISTIC_MODES: [&str; 8] = [
    "bm25-flat-v1-baseline",
    "descriptor-v2-flat-bm25",
    "field-weighted-bm25-v2",
    "normalized-token-bm25-v2",
    "semantic-flat-v2-mean",
    "semantic-field-labelled-v2-mean",
    "rrf-v2",
    "normalized-union-v2",
];

/// Pooling for semantic M002 variants. CLS is the only preregistered
/// ablation; no other pooling may be introduced in M002.
pub const SEMANTIC_POOLINGS: [&str; 2] = ["mean", "cls"];
/// RRF rank constant for `rrf-v2` (M004 value).
pub const RRF_K: f64 = 60.0;
/// Semantic weight for `normalized-union-v2` (equal weighting, M004 parity).
pub const UNION_ALPHA: f64 = 0.5;

// ---- conditional learned-projection grid (M003, frozen here) ----

/// One frozen projection architecture option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionOption {
    pub name: String,
    pub query_tower: String,
    pub descriptor_tower: String,
    pub input_dim: usize,
    pub hidden_dim: Option<usize>,
    pub output_dim: usize,
    pub trainable_params: usize,
}

fn projection_options() -> Vec<ProjectionOption> {
    // Shared linear 384->128: (384*128+128) weights shared across towers.
    let shared = 384 * 128 + 128;
    // Asymmetric linear: one 384->128 head per tower.
    let asymmetric = 2 * (384 * 128 + 128);
    // Asymmetric 2-layer: (384->128->128) per tower.
    let two_layer = 2 * ((384 * 128 + 128) + (128 * 128 + 128));
    vec![
        ProjectionOption {
            name: "shared-linear-384-128".to_string(),
            query_tower: "shared-linear".to_string(),
            descriptor_tower: "shared-linear".to_string(),
            input_dim: MINILM_DIM,
            hidden_dim: None,
            output_dim: PROJECTION_DIM,
            trainable_params: shared,
        },
        ProjectionOption {
            name: "asymmetric-linear-384-128".to_string(),
            query_tower: "linear".to_string(),
            descriptor_tower: "linear".to_string(),
            input_dim: MINILM_DIM,
            hidden_dim: None,
            output_dim: PROJECTION_DIM,
            trainable_params: asymmetric,
        },
        ProjectionOption {
            name: "asymmetric-2layer-384-128-128".to_string(),
            query_tower: "linear-relu-linear".to_string(),
            descriptor_tower: "linear-relu-linear".to_string(),
            input_dim: MINILM_DIM,
            hidden_dim: Some(128),
            output_dim: PROJECTION_DIM,
            trainable_params: two_layer,
        },
    ]
}

/// Frozen projection training grid (M003 only, selected here so M002 cannot
/// expand it after seeing deterministic results).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionTrainingGrid {
    pub learning_rates: Vec<f64>,
    pub epochs: Vec<usize>,
    pub temperatures: Vec<f64>,
    pub loss: String,
    pub seeds: Vec<u64>,
    pub hard_negatives_per_positive: Vec<usize>,
    pub batch_sizes: Vec<usize>,
}

fn projection_training_grid() -> ProjectionTrainingGrid {
    ProjectionTrainingGrid {
        learning_rates: vec![1e-4, 2e-4],
        epochs: vec![5, 10],
        temperatures: vec![0.05, 0.07],
        loss: "infonce".to_string(),
        seeds: vec![7, 42, 123],
        hard_negatives_per_positive: vec![7, 15],
        batch_sizes: vec![64, 128],
    }
}

/// Generic identifier normalization: split snake/kebab/camel boundaries,
/// lowercase, join with single spaces. No per-tool table exists by design;
/// any caller-added alias would violate this preregistration.
pub fn normalize_identifier(name: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in name.chars() {
        if ch == '_' || ch == '-' || ch == '/' || ch == ':' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            continue;
        }
        if ch.is_ascii_uppercase() {
            if !current.is_empty() && current.ends_with(|c: char| c.is_ascii_lowercase()) {
                tokens.push(current.clone());
                current.clear();
            }
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens.join(" ")
}

/// Truncate a field to a byte cap on a char boundary.
pub fn cap_field(value: &str, cap: usize) -> &str {
    if value.len() <= cap {
        return value;
    }
    let mut end = cap;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end()
}

/// Corrected eligible counts per split bound into the receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EligibleCounts {
    pub current_step: usize,
    pub explicit_next_step: usize,
    pub implicit_future: usize,
}

/// The frozen preregistration receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalPreregistration {
    pub schema_version: u16,
    pub protocol: String,
    pub dataset: String,
    pub corpus_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub test_partition_fingerprint: String,
    pub derived_view: String,
    pub derived_view_fingerprint: String,
    pub adjudication_version: String,
    pub train_eligible: EligibleCounts,
    pub dev_eligible: EligibleCounts,
    pub test_eligible: EligibleCounts,
    pub c001_closure: String,
    pub c001_disposition: String,
    pub c001_implementation_commit: String,
    pub gates: BTreeMap<String, f64>,
    pub universes: Vec<usize>,
    pub primary_ks: Vec<usize>,
    pub candidate_fields: Vec<String>,
    pub candidate_field_caps: BTreeMap<String, usize>,
    pub descriptor_total_cap_bytes: usize,
    pub query_fields: Vec<String>,
    pub query_field_caps: BTreeMap<String, usize>,
    pub query_total_cap_bytes: usize,
    pub query_max_next_steps: usize,
    pub field_labels: Vec<String>,
    pub lexical_field_weights: BTreeMap<String, f64>,
    pub deterministic_modes: Vec<String>,
    pub semantic_poolings: Vec<String>,
    pub rrf_k: f64,
    pub union_alpha: f64,
    pub encoder_manifest: String,
    pub ranker_selection: String,
    pub encoder_dim: usize,
    pub projection_dim: usize,
    pub projection_max_params: usize,
    pub projection_options: Vec<ProjectionOption>,
    pub projection_training: ProjectionTrainingGrid,
    pub no_per_tool_aliases: bool,
    pub fingerprint: String,
}

/// Build the frozen receipt from live corpus + derived view plus frozen
/// constants. Fails closed if the corpus or derived view drifted.
pub fn preregistration() -> Result<SignalPreregistration> {
    let cases = super::builtin_cases().context("load builtin corpus")?;
    let full_fp = dataset_fingerprint(&cases).context("fingerprint corpus")?;
    if full_fp != EXPECTED_CORPUS_FINGERPRINT {
        return Err(anyhow!("frozen corpus fingerprint drifted"));
    }
    if RELEVANCE_CORPUS_FP != EXPECTED_CORPUS_FINGERPRINT {
        return Err(anyhow!("C001 corpus binding mismatch"));
    }
    let partition = partition_cases(&cases);
    let fp_for = |indices: &[usize]| -> Result<String> {
        let subset: Vec<super::ToolAdvisorCase> =
            indices.iter().map(|i| cases[*i].clone()).collect();
        dataset_fingerprint(&subset).context("fingerprint split")
    };
    let train_fp = fp_for(&partition.train_cases)?;
    let dev_cases: Vec<super::ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|i| cases[*i].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases).context("fingerprint dev")?;
    if dev_fp != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!("frozen dev partition drifted"));
    }
    if RELEVANCE_DEV_FP != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!("C001 dev binding mismatch"));
    }
    let test_fp = fp_for(&partition.test_cases)?;

    let view = build_derived_view(&cases).context("build derived view")?;
    let derived_fp = derived_view_fingerprint(&view.entries);
    if derived_fp != EXPECTED_DERIVED_VIEW_FINGERPRINT {
        return Err(anyhow!("derived view fingerprint drifted"));
    }

    let impacts = impact_by_split(&view, &cases).context("impact by split")?;
    let counts_for = |split: &str| -> Result<EligibleCounts> {
        let impact = impacts
            .iter()
            .find(|i| i.split == split)
            .ok_or_else(|| anyhow!("missing split {split}"))?;
        Ok(EligibleCounts {
            current_step: impact.current_step,
            explicit_next_step: impact.explicit_next_step,
            implicit_future: impact.implicit_future,
        })
    };
    let train = counts_for("train")?;
    let dev = counts_for("dev")?;
    let test = counts_for("test")?;
    if train.current_step != EXPECTED_TRAIN_CURRENT
        || train.explicit_next_step != EXPECTED_TRAIN_EXPLICIT
        || train.implicit_future != EXPECTED_TRAIN_IMPLICIT
    {
        return Err(anyhow!("train eligible counts drifted"));
    }
    if dev.current_step != EXPECTED_DEV_CURRENT
        || dev.explicit_next_step != EXPECTED_DEV_EXPLICIT
        || dev.implicit_future != EXPECTED_DEV_IMPLICIT
    {
        return Err(anyhow!("dev eligible counts drifted"));
    }
    if test.current_step != EXPECTED_TEST_CURRENT
        || test.explicit_next_step != EXPECTED_TEST_EXPLICIT
        || test.implicit_future != EXPECTED_TEST_IMPLICIT
    {
        return Err(anyhow!("test eligible counts drifted"));
    }

    let mut gates = BTreeMap::new();
    gates.insert("recall_64_min".to_string(), GATE_RECALL_64);
    gates.insert("recall_128_min".to_string(), GATE_RECALL_128);
    gates.insert("recall_256_min".to_string(), GATE_RECALL_256);

    let candidate_fields: Vec<String> = CANDIDATE_FIELDS.iter().map(|s| s.to_string()).collect();
    let candidate_field_caps: BTreeMap<String, usize> = CANDIDATE_FIELD_CAPS
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect();
    let query_fields: Vec<String> = QUERY_FIELDS.iter().map(|s| s.to_string()).collect();
    let query_field_caps: BTreeMap<String, usize> = QUERY_FIELD_CAPS
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect();
    let field_labels: Vec<String> = FIELD_LABELS.iter().map(|s| s.to_string()).collect();
    let lexical_field_weights: BTreeMap<String, f64> = LEXICAL_FIELD_WEIGHTS
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect();
    let deterministic_modes: Vec<String> =
        DETERMINISTIC_MODES.iter().map(|s| s.to_string()).collect();
    let semantic_poolings: Vec<String> = SEMANTIC_POOLINGS.iter().map(|s| s.to_string()).collect();
    let projection_options = projection_options();
    for option in &projection_options {
        if option.trainable_params > PROJECTION_MAX_PARAMS {
            return Err(anyhow!(
                "projection option {} exceeds param cap",
                option.name
            ));
        }
    }

    let mut receipt = SignalPreregistration {
        schema_version: SIGNAL_PREREG_SCHEMA_VERSION,
        protocol: SIGNAL_PREREG_PROTOCOL.to_string(),
        dataset: PREREG_DATASET.to_string(),
        corpus_fingerprint: full_fp,
        train_partition_fingerprint: train_fp,
        dev_partition_fingerprint: dev_fp,
        test_partition_fingerprint: test_fp,
        derived_view: PREREG_DERIVED_VIEW.to_string(),
        derived_view_fingerprint: derived_fp,
        adjudication_version: PREREG_ADJUDICATION_VERSION.to_string(),
        train_eligible: train,
        dev_eligible: dev,
        test_eligible: test,
        c001_closure: C001_CLOSURE_PATH.to_string(),
        c001_disposition: C001_DISPOSITION.to_string(),
        c001_implementation_commit: C001_IMPLEMENTATION_COMMIT.to_string(),
        gates,
        universes: PREREG_UNIVERSES.to_vec(),
        primary_ks: PREREG_PRIMARY_KS.to_vec(),
        candidate_fields,
        candidate_field_caps,
        descriptor_total_cap_bytes: DESCRIPTOR_TOTAL_CAP_BYTES,
        query_fields,
        query_field_caps,
        query_total_cap_bytes: QUERY_TOTAL_CAP_BYTES,
        query_max_next_steps: QUERY_MAX_NEXT_STEPS,
        field_labels,
        lexical_field_weights,
        deterministic_modes,
        semantic_poolings,
        rrf_k: RRF_K,
        union_alpha: UNION_ALPHA,
        encoder_manifest: PREREG_ENCODER_MANIFEST.to_string(),
        ranker_selection: PREREG_RANKER_SELECTION.to_string(),
        encoder_dim: MINILM_DIM,
        projection_dim: PROJECTION_DIM,
        projection_max_params: PROJECTION_MAX_PARAMS,
        projection_options,
        projection_training: projection_training_grid(),
        no_per_tool_aliases: true,
        fingerprint: String::new(),
    };
    receipt.fingerprint = prereg_fingerprint(&receipt)?;
    Ok(receipt)
}

/// Deterministic fingerprint over the receipt (excludes `fingerprint` itself).
pub fn prereg_fingerprint(receipt: &SignalPreregistration) -> Result<String> {
    let mut canonical = serde_json::to_value(receipt).context("serialize prereg")?;
    if let Some(map) = canonical.as_object_mut() {
        map.remove("fingerprint");
    }
    let bytes = serde_json::to_vec(&canonical).context("canonical prereg bytes")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn load_preregistration(path: &Path) -> Result<SignalPreregistration> {
    let bytes = std::fs::read(path).with_context(|| format!("read prereg {}", path.display()))?;
    let receipt: SignalPreregistration = serde_json::from_slice(&bytes).context("parse prereg")?;
    Ok(receipt)
}

pub fn write_preregistration_atomic(path: &Path, receipt: &SignalPreregistration) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create prereg directory {}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(receipt).context("serialize prereg")?;
    std::fs::write(&temp, bytes)
        .with_context(|| format!("write temporary prereg {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("install prereg {}", path.display()))?;
    Ok(())
}

/// Validate a stored receipt against live corpus state and frozen constants.
/// Any derived-view change, count drift, gate relaxation, alias addition, or
/// projection-budget widening fails closed and invalidates the preregistration.
pub fn validate_preregistration(
    receipt: &SignalPreregistration,
    expected_fingerprint: Option<&str>,
) -> Result<()> {
    if receipt.schema_version != SIGNAL_PREREG_SCHEMA_VERSION {
        return Err(anyhow!(
            "prereg schema {} != expected {SIGNAL_PREREG_SCHEMA_VERSION}",
            receipt.schema_version
        ));
    }
    if receipt.protocol != SIGNAL_PREREG_PROTOCOL {
        return Err(anyhow!("prereg protocol mismatch"));
    }
    if receipt.dataset != PREREG_DATASET {
        return Err(anyhow!("prereg dataset mismatch"));
    }
    if receipt.derived_view != PREREG_DERIVED_VIEW {
        return Err(anyhow!("prereg derived view path mismatch"));
    }
    if receipt.adjudication_version != PREREG_ADJUDICATION_VERSION {
        return Err(anyhow!("prereg adjudication mismatch"));
    }
    if receipt.corpus_fingerprint != EXPECTED_CORPUS_FINGERPRINT {
        return Err(anyhow!("prereg corpus fingerprint mismatch"));
    }
    if receipt.dev_partition_fingerprint != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!("prereg dev fingerprint mismatch"));
    }
    if receipt.derived_view_fingerprint != EXPECTED_DERIVED_VIEW_FINGERPRINT {
        return Err(anyhow!("prereg derived view fingerprint mismatch"));
    }
    if receipt.c001_closure != C001_CLOSURE_PATH
        || receipt.c001_disposition != C001_DISPOSITION
        || receipt.c001_implementation_commit != C001_IMPLEMENTATION_COMMIT
    {
        return Err(anyhow!("prereg C001 binding mismatch"));
    }
    if (
        receipt.train_eligible.current_step,
        receipt.train_eligible.explicit_next_step,
        receipt.train_eligible.implicit_future,
    ) != (
        EXPECTED_TRAIN_CURRENT,
        EXPECTED_TRAIN_EXPLICIT,
        EXPECTED_TRAIN_IMPLICIT,
    ) {
        return Err(anyhow!("prereg train counts mismatch"));
    }
    if (
        receipt.dev_eligible.current_step,
        receipt.dev_eligible.explicit_next_step,
        receipt.dev_eligible.implicit_future,
    ) != (
        EXPECTED_DEV_CURRENT,
        EXPECTED_DEV_EXPLICIT,
        EXPECTED_DEV_IMPLICIT,
    ) {
        return Err(anyhow!("prereg dev counts mismatch"));
    }
    if (
        receipt.test_eligible.current_step,
        receipt.test_eligible.explicit_next_step,
        receipt.test_eligible.implicit_future,
    ) != (
        EXPECTED_TEST_CURRENT,
        EXPECTED_TEST_EXPLICIT,
        EXPECTED_TEST_IMPLICIT,
    ) {
        return Err(anyhow!("prereg test counts mismatch"));
    }
    let gate = |id: &str, expected: f64| -> Result<()> {
        let actual = receipt
            .gates
            .get(id)
            .copied()
            .ok_or_else(|| anyhow!("prereg missing gate {id}"))?;
        if (actual - expected).abs() > f64::EPSILON {
            return Err(anyhow!("prereg gate {id} mismatch"));
        }
        Ok(())
    };
    gate("recall_64_min", GATE_RECALL_64)?;
    gate("recall_128_min", GATE_RECALL_128)?;
    gate("recall_256_min", GATE_RECALL_256)?;
    if receipt.universes != PREREG_UNIVERSES.to_vec() {
        return Err(anyhow!("prereg universes mismatch"));
    }
    if receipt.primary_ks != PREREG_PRIMARY_KS.to_vec() {
        return Err(anyhow!("prereg Ks mismatch"));
    }
    let expected_fields: Vec<String> = CANDIDATE_FIELDS.iter().map(|s| s.to_string()).collect();
    if receipt.candidate_fields != expected_fields {
        return Err(anyhow!("prereg candidate fields mismatch"));
    }
    let expected_query: Vec<String> = QUERY_FIELDS.iter().map(|s| s.to_string()).collect();
    if receipt.query_fields != expected_query {
        return Err(anyhow!("prereg query fields mismatch"));
    }
    let expected_modes: Vec<String> = DETERMINISTIC_MODES.iter().map(|s| s.to_string()).collect();
    if receipt.deterministic_modes != expected_modes {
        return Err(anyhow!("prereg mode grid mismatch"));
    }
    if !receipt.no_per_tool_aliases {
        return Err(anyhow!("prereg must forbid per-tool aliases"));
    }
    for option in &receipt.projection_options {
        if option.trainable_params > PROJECTION_MAX_PARAMS
            || option.trainable_params > receipt.projection_max_params
        {
            return Err(anyhow!(
                "prereg projection option {} exceeds param cap",
                option.name
            ));
        }
    }
    if receipt.projection_dim != PROJECTION_DIM || receipt.encoder_dim != MINILM_DIM {
        return Err(anyhow!("prereg encoder/projection dim mismatch"));
    }
    let recomputed = prereg_fingerprint(receipt)?;
    if receipt.fingerprint != recomputed {
        return Err(anyhow!("prereg fingerprint mismatch"));
    }
    if let Some(expected) = expected_fingerprint {
        if receipt.fingerprint != expected {
            return Err(anyhow!("prereg fingerprint drifted from frozen receipt"));
        }
    }
    // Live binding: the frozen receipt must still match a fresh build.
    let fresh = preregistration()?;
    if fresh.corpus_fingerprint != receipt.corpus_fingerprint
        || fresh.derived_view_fingerprint != receipt.derived_view_fingerprint
        || fresh.dev_partition_fingerprint != receipt.dev_partition_fingerprint
        || fresh.train_eligible != receipt.train_eligible
        || fresh.dev_eligible != receipt.dev_eligible
        || fresh.test_eligible != receipt.test_eligible
    {
        return Err(anyhow!("prereg live target binding drifted"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_advisor::retrieval_relevance::load_derived_view;

    #[test]
    fn frozen_target_binding_matches_c001() {
        let receipt = preregistration().expect("prereg");
        assert_eq!(receipt.corpus_fingerprint, EXPECTED_CORPUS_FINGERPRINT);
        assert_eq!(
            receipt.dev_partition_fingerprint,
            EXPECTED_DEV_PARTITION_FINGERPRINT
        );
        assert_eq!(
            receipt.derived_view_fingerprint,
            EXPECTED_DERIVED_VIEW_FINGERPRINT
        );
        assert_eq!(
            (
                receipt.dev_eligible.current_step,
                receipt.dev_eligible.implicit_future
            ),
            (EXPECTED_DEV_CURRENT, EXPECTED_DEV_IMPLICIT)
        );
        assert_eq!(receipt.c001_disposition, "B");
    }

    #[test]
    fn committed_derived_view_matches_binding() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join(PREREG_DERIVED_VIEW);
        if std::fs::read(&path).is_err() {
            eprintln!("SKIP: derived view asset absent");
            return;
        }
        let stored = load_derived_view(&path).expect("parse derived view");
        let recomputed = derived_view_fingerprint(&stored.entries);
        assert_eq!(recomputed, EXPECTED_DERIVED_VIEW_FINGERPRINT);
        assert_eq!(stored.corpus_fingerprint, EXPECTED_CORPUS_FINGERPRINT);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let first = preregistration().expect("prereg");
        let second = preregistration().expect("prereg");
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first, second);
    }

    #[test]
    fn identifier_normalization_is_generic() {
        // Known persistent misses decompose without a per-tool table.
        assert_eq!(normalize_identifier("glob"), "glob");
        assert_eq!(normalize_identifier("lsp_rename"), "lsp rename");
        assert_eq!(normalize_identifier("table_filter"), "table filter");
        assert_eq!(normalize_identifier("git_log"), "git log");
        // Generic rules apply to unseen names identically.
        assert_eq!(normalize_identifier("myNewTool"), "my new tool");
        assert_eq!(normalize_identifier("some-kebab-tool"), "some kebab tool");
        assert_eq!(normalize_identifier("read"), "read");
        // No alias table exists: the function is pure string splitting.
        assert_eq!(normalize_identifier("write"), "write");
    }

    #[test]
    fn field_caps_and_weights_are_frozen() {
        let receipt = preregistration().expect("prereg");
        assert_eq!(
            receipt.descriptor_total_cap_bytes,
            DESCRIPTOR_TOTAL_CAP_BYTES
        );
        assert_eq!(receipt.query_total_cap_bytes, QUERY_TOTAL_CAP_BYTES);
        assert_eq!(receipt.query_max_next_steps, QUERY_MAX_NEXT_STEPS);
        assert_eq!(receipt.lexical_field_weights["canonical_name"], 3.0);
        assert_eq!(receipt.deterministic_modes.len(), DETERMINISTIC_MODES.len());
        assert_eq!(receipt.semantic_poolings, vec!["mean", "cls"]);
        assert!((receipt.rrf_k - 60.0).abs() < f64::EPSILON);
        assert!((receipt.union_alpha - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn projection_grid_respects_param_cap() {
        let receipt = preregistration().expect("prereg");
        assert_eq!(receipt.projection_options.len(), 3);
        for option in &receipt.projection_options {
            assert!(option.trainable_params <= PROJECTION_MAX_PARAMS);
        }
        // Exact hand-computed parameter counts (bias included).
        let by_name: BTreeMap<&str, usize> = receipt
            .projection_options
            .iter()
            .map(|o| (o.name.as_str(), o.trainable_params))
            .collect();
        assert_eq!(by_name["shared-linear-384-128"], 384 * 128 + 128);
        assert_eq!(by_name["asymmetric-linear-384-128"], 2 * (384 * 128 + 128));
        assert_eq!(
            by_name["asymmetric-2layer-384-128-128"],
            2 * ((384 * 128 + 128) + (128 * 128 + 128))
        );
        assert_eq!(receipt.projection_training.loss, "infonce");
        assert_eq!(receipt.projection_training.seeds, vec![7, 42, 123]);
    }

    #[test]
    fn no_per_tool_aliases_allowed() {
        let receipt = preregistration().expect("prereg");
        assert!(receipt.no_per_tool_aliases);
        // The receipt carries no alias map by construction.
        let value = serde_json::to_value(&receipt).expect("serialize");
        assert!(value.get("aliases").is_none());
        assert!(value.get("per_tool_aliases").is_none());
    }

    #[test]
    fn gates_are_not_relaxed() {
        let receipt = preregistration().expect("prereg");
        assert!((receipt.gates["recall_64_min"] - 0.99).abs() < f64::EPSILON);
        assert!((receipt.gates["recall_128_min"] - 0.98).abs() < f64::EPSILON);
        assert!((receipt.gates["recall_256_min"] - 0.95).abs() < f64::EPSILON);
        assert_eq!(receipt.universes, vec![64, 128, 256]);
        assert_eq!(receipt.primary_ks, vec![16, 24, 32]);
    }

    #[test]
    fn committed_receipt_matches_live_build() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join(SIGNAL_PREREG_ASSET_PATH);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("SKIP: prereg receipt not yet generated");
            return;
        };
        let stored: SignalPreregistration = serde_json::from_slice(&bytes).expect("parse receipt");
        let fresh = preregistration().expect("fresh prereg");
        assert_eq!(
            stored, fresh,
            "committed receipt drifted from frozen contract"
        );
        validate_preregistration(&stored, None).expect("validate receipt");
    }

    #[test]
    fn tampered_receipt_fails_closed() {
        let mut receipt = preregistration().expect("prereg");
        receipt.gates.insert("recall_64_min".to_string(), 0.90);
        assert!(validate_preregistration(&receipt, None).is_err());
        let mut receipt = preregistration().expect("prereg");
        receipt.no_per_tool_aliases = false;
        assert!(validate_preregistration(&receipt, None).is_err());
        let mut receipt = preregistration().expect("prereg");
        receipt.derived_view_fingerprint = "deadbeef".to_string();
        assert!(validate_preregistration(&receipt, None).is_err());
    }

    /// Generate the repository-owned preregistration receipt. Run explicitly:
    /// `cargo test --locked -p codegg --lib --
    ///  tool_advisor::retrieval_signal::tests::generate_prereg_asset --
    ///  --ignored --nocapture`
    #[test]
    #[ignore]
    fn generate_prereg_asset() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let receipt = preregistration().expect("prereg");
        validate_preregistration(&receipt, None).expect("validate");
        let path = root.join(SIGNAL_PREREG_ASSET_PATH);
        write_preregistration_atomic(&path, &receipt).expect("write receipt");
        eprintln!(
            "wrote {} fingerprint={} derived={} dev={}/{}/{}",
            path.display(),
            receipt.fingerprint,
            receipt.derived_view_fingerprint,
            receipt.dev_eligible.current_step,
            receipt.dev_eligible.explicit_next_step,
            receipt.dev_eligible.implicit_future
        );
    }
}
