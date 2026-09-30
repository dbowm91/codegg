//! M003 preregistered frozen-encoder retrieval projection experiment.
//!
//! This module is offline experiment infrastructure. MiniLM vectors are
//! cached as immutable inputs; only the small projection heads receive
//! gradients. It never participates in the production advisor path.

use super::retrieval_signal::{
    extract_schema_fields, live_builtin_schema, RetrievalDescriptorV2, RetrievalQueryV2,
    GATE_RECALL_128, GATE_RECALL_256, GATE_RECALL_64, MAX_PROJECTION_PARAMS, PREREG_PRIMARY_KS,
    PREREG_UNIVERSES, PROJECTION_BATCH_SIZE, PROJECTION_FAMILIES,
    PROJECTION_HARD_NEGATIVES_PER_POSITIVE, PROJECTION_LR_GRID, PROJECTION_MAX_EPOCHS,
    PROJECTION_SEEDS, PROJECTION_TEMPERATURE_GRID,
};
use super::retrieval_signal_m002::{current_step_dev_cases, M002_ENCODER_MANIFEST};
use super::retrieval_signal_m003_audit::audited_optimizer_cases;
use super::sequence_encoder::{CandleBertSequenceEncoder, PoolingStrategy};
use super::{
    dataset_fingerprint, load_cases, partition_cases, ToolAdvisorCandidate, ToolAdvisorCase,
};
use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor, D};
use candle_nn::{linear, AdamW, Linear, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const M003_PROTOCOL: &str = "retrieval-signal-m003-projection-sweep-v1";
pub const M003_RECEIPT_PATH: &str = "assets/tool-advisor/retrieval-signal-m003-projection.json";
pub const M003_HARD_NEGATIVE_PATH: &str =
    "assets/tool-advisor/retrieval-signal-m003-hard-negatives.json";
pub const M003_OUTPUT_DIR: &str = "target/tool-advisor/retrieval-signal-m003";
const HIDDEN: usize = 384;
const PROJECTED: usize = 128;
const M006_DECISION_SHA256: &str =
    "24e3783f1cc1c3d8934bac1592607702966112554db8ca1215087f90cfd09756";
const TRAIN_AUDIT_SHA256: &str = "b761dba49d6ec6324b28a6cc8a4ec36af06b399fabe8438dfab0457c86da3193";
const M002_RECEIPT_SHA256: &str =
    "7045d4fccb900c4b0f17dfb7f28e92d2c60d05385b5892d2cffe7145d7dd0a05";
const ENCODER_MANIFEST_SHA256: &str =
    "e671e6876111ff9e0be190695a92332880c8bc352a30ee9542da4d4916a9bf3d";
const HARD_NEGATIVE_SHA256: &str =
    "17e6c1b2f3db1517a33d9e2df2770dd60ec60833cca5e2598e4c372834d7ed92";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M003Point {
    pub family: String,
    pub learning_rate: f64,
    pub temperature: f64,
    pub seed: u64,
    pub epochs: u32,
    pub parameters: u64,
    pub train_loss: Vec<f64>,
    pub recall: BTreeMap<String, f64>,
    pub primary_recall: BTreeMap<String, f64>,
    pub secondary_recall: BTreeMap<String, f64>,
    pub mrr: BTreeMap<String, f64>,
    pub per_tool_recall_u64_k32: BTreeMap<String, f64>,
    pub persistent_miss_recall_u64_k32: BTreeMap<String, f64>,
    pub family_recall: BTreeMap<String, f64>,
    pub unknown_renamed_mrr: f64,
    pub name_masked_recall_u64_k32: f64,
    pub authority_violations: usize,
    pub generalization_pass: bool,
    pub selected_k: Option<usize>,
    pub query_projection_latency_p50_ms: f64,
    pub descriptor_projection_cache_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M003Receipt {
    pub schema_version: u16,
    pub protocol: String,
    pub m001_preregistration_sha256: String,
    pub m002_receipt_sha256: String,
    pub m006_decision_sha256: String,
    pub train_audit_sha256: String,
    pub dataset_sha256: String,
    pub train_partition_sha256: String,
    pub dev_partition_sha256: String,
    pub encoder_manifest_sha256: String,
    pub encoder_weights_sha256: String,
    pub encoder_vocabulary_sha256: String,
    pub representation_schema_sha256: String,
    pub projection_contract_sha256: String,
    pub baseline_family_recall_u64_k32: BTreeMap<String, f64>,
    pub baseline_unknown_renamed_mrr: f64,
    pub name_masked_lexical_baseline_recall_u64_k32: f64,
    pub optimizer_input_sha256: String,
    pub hard_negative_sha256: String,
    pub cases_train: usize,
    pub cases_dev: usize,
    pub dev_no_tool_cases: usize,
    pub grid_points: Vec<M003Point>,
    pub selected_point: Option<usize>,
    pub disposition: String,
    pub selected_artifact_sha256: Option<String>,
    pub selected_artifact_bytes: Option<u64>,
    pub incremental_rss_bytes: Option<u64>,
}

#[derive(Clone)]
struct Pair {
    case_id: String,
    tool: String,
    grade: u8,
    query: Vec<f32>,
    positive: Vec<f32>,
    negatives: Vec<(String, Vec<f32>, bool)>,
}

#[derive(Debug, Clone)]
struct RankedCase {
    case: ToolAdvisorCase,
    query: Vec<f32>,
    descriptors: Vec<(ToolAdvisorCandidate, Vec<f32>)>,
}

struct Head {
    varmap: VarMap,
    query_1: Linear,
    query_2: Option<Linear>,
    descriptor_1: Option<Linear>,
    descriptor_2: Option<Linear>,
    shared: bool,
    device: Device,
}

fn descriptor(candidate: &ToolAdvisorCandidate) -> RetrievalDescriptorV2 {
    let schema = live_builtin_schema(&candidate.name)
        .map(|value| extract_schema_fields(&value))
        .unwrap_or_default();
    RetrievalDescriptorV2::from_candidate(candidate, schema)
}

fn query_text(case: &ToolAdvisorCase) -> String {
    RetrievalQueryV2::from_advisor_context(&super::retrieval_signal_m002::m002_context(case))
        .flat_text()
}

fn descriptor_text(candidate: &ToolAdvisorCandidate) -> String {
    descriptor(candidate).flat_text()
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn digest_file(path: impl AsRef<Path>) -> Result<String> {
    Ok(digest(&fs::read(path.as_ref())?))
}

fn projection_contract() -> serde_json::Value {
    serde_json::json!({
        "protocol": M003_PROTOCOL,
        "input": {
            "query": "RetrievalQueryV2::flat_text",
            "descriptor": "RetrievalDescriptorV2::flat_text",
            "encoder": "pinned all-MiniLM-L6-v2 frozen",
            "pooling": "mean",
            "normalization": "l2 before cosine"
        },
        "training": {
            "optimizer": "AdamW",
            "weight_decay": 0.0,
            "initialization": "seeded xorshift uniform [-sqrt(6/fan_in),sqrt(6/fan_in)]; zero bias",
            "ordering": "train partition order; per-epoch seeded Fisher-Yates",
            "contrastive": "in-batch multi-positive log-softmax; same tool identities are positives",
            "graded_weights": {"grade_2": 0.5, "grade_3": 1.0},
            "hard_negative_pool": "union of candidate identities in the filtered historical train partition",
            "hard_negative_mining": "rank sum of current BM25 and frozen raw MiniLM cosine; reserve best same-family candidate when present; seven distinct identities",
            "hard_negative_loss": "mean relu(0.2 - positive_cosine + negative_cosine), weight 0.5",
            "same_family_loss": "same hinge, weight 0.1",
            "no_tool_examples": "omitted from projection optimization",
            "epochs": PROJECTION_MAX_EPOCHS,
            "batch_size": PROJECTION_BATCH_SIZE
        },
        "selection": {
            "universes": PREREG_UNIVERSES,
            "k": PREREG_PRIMARY_KS,
            "recall_gates": {"64": GATE_RECALL_64, "128": GATE_RECALL_128, "256": GATE_RECALL_256},
            "unknown_renamed_mrr_floor_delta": 0.02,
            "family_recall_floor_delta": 0.03,
            "name_masked": "must exceed lexical description-only recall",
            "tie_break": ["fewest_parameters", "lowest_passing_k", "lowest_projection_latency"]
        }
    })
}

fn projection_contract_sha256() -> Result<String> {
    Ok(digest(&serde_json::to_vec(&projection_contract())?))
}

fn l2_normalize(input: &Tensor) -> Result<Tensor> {
    let norm = input
        .sqr()?
        .sum(D::Minus1)?
        .sqrt()?
        .clamp(1e-12, f64::MAX)?;
    Ok(input.broadcast_div(&norm.unsqueeze(D::Minus1)?)?)
}

impl Head {
    fn new(family: &str, seed: u64, device: &Device) -> Result<Self> {
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, device);
        let shared = family == "shared-linear-384x128";
        let mlp = family == "asymmetric-mlp-384x128x128";
        if !shared && !mlp && family != "asymmetric-linear-384x128" {
            return Err(anyhow!("unknown M003 projection family {family}"));
        }
        let query_1 = linear(HIDDEN, PROJECTED, vb.pp("query_1"))?;
        let descriptor_1 = if shared {
            None
        } else {
            Some(linear(HIDDEN, PROJECTED, vb.pp("descriptor_1"))?)
        };
        let query_2 = if mlp {
            Some(linear(PROJECTED, PROJECTED, vb.pp("query_2"))?)
        } else {
            None
        };
        let descriptor_2 = if mlp {
            Some(linear(PROJECTED, PROJECTED, vb.pp("descriptor_2"))?)
        } else {
            None
        };
        // Candle's CPU generator intentionally cannot be seeded. Seed every
        // parameter explicitly so the preregistered seed is host-independent.
        let mut state = seed ^ 0x9e3779b97f4a7c15;
        let vars = varmap.data().lock().expect("projection varmap lock");
        let mut ordered_vars = vars.iter().collect::<Vec<_>>();
        ordered_vars.sort_by(|left, right| left.0.cmp(right.0));
        for (name, var) in ordered_vars {
            let shape = var.as_tensor().shape().clone();
            let dims = shape.dims();
            let count = shape.elem_count();
            let mut values = Vec::with_capacity(count);
            if name.ends_with("bias") {
                values.resize(count, 0.0f32);
            } else {
                let fan_in = dims.last().copied().unwrap_or(HIDDEN).max(1) as f32;
                let scale = (6.0 / fan_in).sqrt();
                for _ in 0..count {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let unit = (state as u32) as f32 / u32::MAX as f32;
                    values.push((unit * 2.0 - 1.0) * scale);
                }
            }
            var.set(&Tensor::from_vec(values, shape, device)?)?;
        }
        drop(vars);
        Ok(Self {
            varmap,
            query_1,
            query_2,
            descriptor_1,
            descriptor_2,
            shared,
            device: device.clone(),
        })
    }

    fn project(&self, input: &Tensor, query: bool) -> Result<Tensor> {
        let first = if query || self.shared {
            self.query_1.forward(input)?
        } else {
            self.descriptor_1
                .as_ref()
                .expect("asymmetric descriptor head")
                .forward(input)?
        };
        let second = if query || self.shared {
            self.query_2.as_ref()
        } else {
            self.descriptor_2.as_ref()
        };
        match second {
            Some(layer) => l2_normalize(&layer.forward(&first.relu()?)?),
            None => l2_normalize(&first),
        }
    }

    fn parameter_count(&self) -> u64 {
        self.varmap
            .data()
            .lock()
            .expect("projection varmap lock")
            .values()
            .map(|var| var.as_tensor().elem_count() as u64)
            .sum()
    }

    fn scores(&self, query: &[f32], descriptors: &[Vec<f32>]) -> Result<Vec<f64>> {
        let q = self.project(&Tensor::new(query, &self.device)?.unsqueeze(0)?, true)?;
        let d = self.project(&tensor_rows(descriptors, &self.device)?, false)?;
        Ok(q.matmul(&d.t()?)?.to_vec2::<f32>()?[0]
            .iter()
            .map(|score| *score as f64)
            .collect())
    }
}

fn build_pairs(
    cases: &[ToolAdvisorCase],
    encoder: &CandleBertSequenceEncoder,
    vectors: &mut HashMap<String, Vec<f32>>,
) -> Result<Vec<Pair>> {
    // Mine against the union of candidate identities present in the frozen
    // train partition. A single train row often has fewer than seven
    // distractors; using the train-only surface supplies the preregistered
    // seven identities without importing examples or labels from another split.
    let mut global_candidates = BTreeMap::<String, ToolAdvisorCandidate>::new();
    for case in cases {
        for candidate in &case.candidates {
            global_candidates
                .entry(candidate.name.clone())
                .or_insert_with(|| {
                    let mut deferred = candidate.clone();
                    deferred.disclosure = "deferred".into();
                    deferred
                });
        }
    }
    let global_metadata = global_candidates
        .values()
        .map(|candidate| crate::tool::catalog::ToolMetadata {
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            parameters: serde_json::Value::Null,
            defer_load: true,
            category: candidate.category.clone(),
            disclosure: "deferred".into(),
        })
        .collect::<Vec<_>>();
    let mut global_vectors = BTreeMap::new();
    for candidate in global_candidates.values() {
        let text = descriptor_text(candidate);
        let vector = if let Some(vector) = vectors.get(&text) {
            vector.clone()
        } else {
            let vector = encoder.encode_context(&text, PoolingStrategy::Mean)?;
            vectors.insert(text, vector.clone());
            vector
        };
        global_vectors.insert(candidate.name.clone(), vector);
    }
    let mut pairs = Vec::new();
    for case in cases {
        let query = query_text(case);
        let query_vec = encoder.encode_context(&query, PoolingStrategy::Mean)?;
        let lexical = crate::tool::catalog::ToolCatalog::rank_descriptors(
            &case.context,
            &global_metadata,
            crate::tool::catalog::SearchMode::BM25,
        )
        .into_iter()
        .map(|candidate| candidate.name)
        .collect::<Vec<_>>();
        let lexical_rank = lexical
            .iter()
            .enumerate()
            .map(|(rank, name)| (name.clone(), rank))
            .collect::<BTreeMap<_, _>>();
        let mut semantic = global_vectors
            .iter()
            .map(|(name, vector)| (name.clone(), cosine(&query_vec, vector)))
            .collect::<Vec<_>>();
        semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let semantic_rank = semantic
            .iter()
            .enumerate()
            .map(|(rank, (name, _))| (name.clone(), rank))
            .collect::<BTreeMap<_, _>>();
        for (tool, grade) in &case.relevance {
            case.candidates
                .iter()
                .find(|candidate| &candidate.name == tool)
                .with_context(|| {
                    format!("positive candidate {tool} missing in {}", case.case_id)
                })?;
            let mut positive_candidate = case
                .candidates
                .iter()
                .find(|candidate| candidate.name == *tool)
                .with_context(|| format!("positive descriptor {tool} missing in train case"))?
                .clone();
            positive_candidate.disclosure = "deferred".into();
            let positive_text = descriptor_text(&positive_candidate);
            let positive = if let Some(vector) = vectors.get(&positive_text) {
                vector.clone()
            } else {
                let vector = encoder.encode_context(&positive_text, PoolingStrategy::Mean)?;
                vectors.insert(positive_text, vector.clone());
                vector
            };
            let mut negative_names = global_candidates
                .keys()
                .filter(|name| *name != tool && !case.relevance.contains_key(*name))
                .cloned()
                .collect::<Vec<_>>();
            negative_names.sort_by(|a, b| {
                lexical_rank
                    .get(a)
                    .copied()
                    .unwrap_or(usize::MAX / 2)
                    .saturating_add(semantic_rank.get(a).copied().unwrap_or(usize::MAX / 2))
                    .cmp(
                        &lexical_rank
                            .get(b)
                            .copied()
                            .unwrap_or(usize::MAX / 2)
                            .saturating_add(
                                semantic_rank.get(b).copied().unwrap_or(usize::MAX / 2),
                            ),
                    )
                    .then_with(|| a.cmp(b))
            });
            let family = tool.split('_').next().unwrap_or(tool);
            let family_negative = negative_names
                .iter()
                .find(|name| name.split('_').next().unwrap_or(name) == family)
                .cloned();
            let mut negatives = Vec::new();
            if let Some(name) = &family_negative {
                negatives.push((name.clone(), global_vectors[name].clone(), true));
            }
            for name in negative_names {
                if negatives.len() == PROJECTION_HARD_NEGATIVES_PER_POSITIVE {
                    break;
                }
                if negatives.iter().any(|(existing, _, _)| existing == &name) {
                    continue;
                }
                let vector = global_vectors
                    .get(&name)
                    .with_context(|| format!("frozen train negative {name} lacks an embedding"))?
                    .clone();
                let same_family = name.split('_').next().unwrap_or(&name) == family;
                negatives.push((name, vector, same_family));
            }
            if negatives.len() < PROJECTION_HARD_NEGATIVES_PER_POSITIVE {
                continue;
            }
            negatives.truncate(PROJECTION_HARD_NEGATIVES_PER_POSITIVE);
            pairs.push(Pair {
                case_id: case.case_id.clone(),
                tool: tool.clone(),
                grade: *grade,
                query: query_vec.clone(),
                positive,
                negatives,
            });
        }
    }
    Ok(pairs)
}

fn cosine(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum()
}

fn prepare_dev(
    dev: &[ToolAdvisorCase],
    encoder: &CandleBertSequenceEncoder,
    vectors: &mut HashMap<String, Vec<f32>>,
) -> Result<Vec<RankedCase>> {
    let mut outputs = Vec::new();
    for size in PREREG_UNIVERSES {
        for case in super::operating_point::expand_universe(dev, size)? {
            let query = query_text(&case);
            let query = encoder.encode_context(&query, PoolingStrategy::Mean)?;
            let mut descriptors = Vec::new();
            for candidate in &case.candidates {
                if candidate.disclosure != "deferred" {
                    continue;
                }
                let text = descriptor_text(candidate);
                let vector = if let Some(vector) = vectors.get(&text) {
                    vector.clone()
                } else {
                    let vector = encoder.encode_context(&text, PoolingStrategy::Mean)?;
                    vectors.insert(text, vector.clone());
                    vector
                };
                descriptors.push((candidate.clone(), vector));
            }
            outputs.push(RankedCase {
                case,
                query,
                descriptors,
            });
        }
    }
    Ok(outputs)
}

fn tensor_rows(rows: &[Vec<f32>], device: &Device) -> Result<Tensor> {
    let width = rows.first().map(Vec::len).unwrap_or(HIDDEN);
    let data = rows.iter().flat_map(|row| row.iter().copied()).collect();
    Ok(Tensor::from_vec(data, (rows.len(), width), device)?)
}

fn train_head(
    head: &Head,
    pairs: &[Pair],
    learning_rate: f64,
    temperature: f64,
    seed: u64,
) -> Result<Vec<f64>> {
    let mut optimizer = AdamW::new(
        head.varmap.all_vars(),
        ParamsAdamW {
            lr: learning_rate,
            weight_decay: 0.0,
            ..Default::default()
        },
    )?;
    let mut losses = Vec::new();
    let mut rng = seed ^ 0xa0761d6478bd642f;
    for _epoch in 0..PROJECTION_MAX_EPOCHS {
        let mut order: Vec<usize> = (0..pairs.len()).collect();
        // Deterministic Fisher-Yates shuffle, independently reproducible per seed.
        for index in (1..order.len()).rev() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            order.swap(index, (rng as usize) % (index + 1));
        }
        let mut epoch_loss = 0.0;
        let mut batches = 0usize;
        for indexes in order.chunks(PROJECTION_BATCH_SIZE) {
            if indexes.len() < 2 {
                continue;
            }
            let qs = indexes
                .iter()
                .map(|index| pairs[*index].query.clone())
                .collect::<Vec<_>>();
            let ds = indexes
                .iter()
                .map(|index| pairs[*index].positive.clone())
                .collect::<Vec<_>>();
            let q = head.project(&tensor_rows(&qs, &head.device)?, true)?;
            let d = head.project(&tensor_rows(&ds, &head.device)?, false)?;
            let logits = q.matmul(&d.t()?)?.affine(1.0 / temperature, 0.0)?;
            let log_probs = candle_nn::ops::log_softmax(&logits, 1)?;
            let mut row_losses = Vec::new();
            let mut hard_losses = Vec::new();
            let mut family_losses = Vec::new();
            for (row, pair_index) in indexes.iter().enumerate() {
                let pair = &pairs[*pair_index];
                let same_tool = indexes
                    .iter()
                    .enumerate()
                    .filter(|(_, other)| pairs[**other].tool == pair.tool)
                    .map(|(column, _)| column)
                    .collect::<Vec<_>>();
                let log_row = log_probs.narrow(0, row, 1)?;
                let mut positives = Vec::new();
                for column in same_tool {
                    positives.push(log_row.narrow(1, column, 1)?.squeeze(0)?.squeeze(0)?);
                }
                let positive_log = Tensor::stack(&positives, 0)?.mean_all()?;
                let grade_weight = if pair.grade >= 3 { 1.0 } else { 0.5 };
                row_losses.push((positive_log.neg()? * grade_weight as f64)?);
                // The seven train-only hard negatives are frozen before any
                // grid fit. Their mean hinge term implements M001's 0.5 term.
                if !pair.negatives.is_empty() {
                    let qrow = q.narrow(0, row, 1)?;
                    let positive_score =
                        qrow.broadcast_mul(&d.narrow(0, row, 1)?)?.sum(D::Minus1)?;
                    let mut neg_scores = Vec::new();
                    let mut family_scores = Vec::new();
                    for (name, vector, same_family) in &pair.negatives {
                        let neg = head.project(
                            &Tensor::new(vector.as_slice(), &head.device)?.unsqueeze(0)?,
                            false,
                        )?;
                        let score = qrow.broadcast_mul(&neg)?.sum(D::Minus1)?;
                        let hinge = (Tensor::new(&[0.2f32], &head.device)? - &positive_score
                            + &score)?
                            .relu()?
                            .mean_all()?;
                        if *same_family {
                            family_scores.push(hinge.clone());
                        }
                        let _ = name;
                        neg_scores.push(hinge);
                    }
                    if !neg_scores.is_empty() {
                        hard_losses.push(Tensor::stack(&neg_scores, 0)?.mean_all()?);
                    }
                    if !family_scores.is_empty() {
                        family_losses.push(Tensor::stack(&family_scores, 0)?.mean_all()?);
                    }
                }
            }
            let contrastive = Tensor::stack(&row_losses, 0)?.mean_all()?;
            let mut loss = contrastive;
            if !hard_losses.is_empty() {
                loss = (loss
                    + Tensor::stack(&hard_losses, 0)?
                        .mean_all()?
                        .affine(0.5, 0.0)?)?;
            }
            if !family_losses.is_empty() {
                loss = (loss
                    + Tensor::stack(&family_losses, 0)?
                        .mean_all()?
                        .affine(0.1, 0.0)?)?;
            }
            let value = loss.to_vec0::<f32>()? as f64;
            if !value.is_finite() {
                return Err(anyhow!("M003 training loss became non-finite"));
            }
            optimizer.backward_step(&loss)?;
            epoch_loss += value;
            batches += 1;
        }
        losses.push(if batches == 0 {
            0.0
        } else {
            epoch_loss / batches as f64
        });
    }
    Ok(losses)
}

#[derive(Default)]
struct MetricAccum {
    relevant: usize,
    recovered: usize,
    reciprocal_rank: f64,
    ranked_relevant: usize,
    family: BTreeMap<String, (usize, usize)>,
    primary_relevant: usize,
    primary_recovered: usize,
    secondary_relevant: usize,
    secondary_recovered: usize,
    per_tool: BTreeMap<String, (usize, usize)>,
}

fn evaluate(
    head: &Head,
    cases: &[RankedCase],
    ks: &[usize],
) -> Result<(
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
    usize,
    f64,
    f64,
)> {
    let mut metrics = BTreeMap::<String, MetricAccum>::new();
    let mut violations = 0;
    let mut query_times = Vec::new();
    let mut descriptor_projection_ms = 0.0;
    for ranked_case in cases {
        let universe = ranked_case.descriptors.len();
        let raw = ranked_case
            .descriptors
            .iter()
            .map(|(_, vector)| vector.clone())
            .collect::<Vec<_>>();
        let query_start = Instant::now();
        let q = head.project(
            &Tensor::new(ranked_case.query.as_slice(), &head.device)?.unsqueeze(0)?,
            true,
        )?;
        query_times.push(query_start.elapsed().as_secs_f64() * 1000.0);
        let descriptor_start = Instant::now();
        let d = head.project(&tensor_rows(&raw, &head.device)?, false)?;
        descriptor_projection_ms += descriptor_start.elapsed().as_secs_f64() * 1000.0;
        let values = q.matmul(&d.t()?)?.to_vec2::<f32>()?[0]
            .iter()
            .map(|score| *score as f64)
            .collect::<Vec<_>>();
        let mut scores = ranked_case
            .descriptors
            .iter()
            .zip(values)
            .map(|((candidate, _), score)| (candidate.name.clone(), score))
            .collect::<Vec<_>>();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for k in ks {
            let key = format!("u{universe}-k{k}");
            let metric = metrics.entry(key).or_default();
            let top = scores
                .iter()
                .take(*k)
                .map(|(name, _)| name.as_str())
                .collect::<BTreeSet<_>>();
            let allowed = ranked_case
                .descriptors
                .iter()
                .map(|(candidate, _)| candidate.name.as_str())
                .collect::<BTreeSet<_>>();
            violations += scores
                .iter()
                .take(*k)
                .filter(|(name, _)| !allowed.contains(name.as_str()))
                .count();
            for tool in ranked_case.case.relevance.keys() {
                metric.relevant += 1;
                let found = top.contains(tool.as_str());
                metric.recovered += usize::from(found);
                let max_grade = ranked_case
                    .case
                    .relevance
                    .values()
                    .copied()
                    .max()
                    .unwrap_or(0);
                if ranked_case.case.relevance[tool] == max_grade {
                    metric.primary_relevant += 1;
                    metric.primary_recovered += usize::from(found);
                } else {
                    metric.secondary_relevant += 1;
                    metric.secondary_recovered += usize::from(found);
                }
                let tool_counts = metric.per_tool.entry(tool.clone()).or_default();
                tool_counts.0 += 1;
                tool_counts.1 += usize::from(found);
                if let Some(rank) = scores.iter().position(|(name, _)| name == tool) {
                    metric.reciprocal_rank += 1.0 / (rank + 1) as f64;
                    metric.ranked_relevant += 1;
                }
                if let Some(candidate) = ranked_case
                    .descriptors
                    .iter()
                    .find(|(item, _)| &item.name == tool)
                    .map(|(item, _)| item)
                {
                    let family = tool.split('_').next().unwrap_or(tool).to_string();
                    let entry = metric.family.entry(family).or_default();
                    entry.0 += 1;
                    entry.1 += usize::from(found);
                    let _ = candidate;
                }
            }
        }
    }
    let recalls = metrics
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.recovered as f64 / value.relevant.max(1) as f64,
            )
        })
        .collect();
    let primary = metrics
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.primary_recovered as f64 / value.primary_relevant.max(1) as f64,
            )
        })
        .collect();
    let secondary = metrics
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.secondary_recovered as f64 / value.secondary_relevant.max(1) as f64,
            )
        })
        .collect();
    let mrr = metrics
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.reciprocal_rank / value.ranked_relevant.max(1) as f64,
            )
        })
        .collect();
    let family = metrics
        .get("u64-k32")
        .map(|metric| {
            metric
                .family
                .iter()
                .map(|(key, (relevant, recovered))| {
                    (key.clone(), *recovered as f64 / (*relevant).max(1) as f64)
                })
                .collect()
        })
        .unwrap_or_default();
    let per_tool: BTreeMap<String, f64> = metrics
        .get("u64-k32")
        .map(|metric| {
            metric
                .per_tool
                .iter()
                .map(|(key, (relevant, recovered))| {
                    (key.clone(), *recovered as f64 / (*relevant).max(1) as f64)
                })
                .collect()
        })
        .unwrap_or_default();
    let persistent: BTreeMap<String, f64> = per_tool
        .iter()
        .filter(|(tool, _)| super::retrieval_signal::PERSISTENT_MISS_TOOLS.contains(&tool.as_str()))
        .map(|(tool, recall)| (tool.clone(), *recall))
        .collect();
    query_times.sort_by(f64::total_cmp);
    let query_p50 = if query_times.is_empty() {
        0.0
    } else {
        query_times[query_times.len() / 2]
    };
    Ok((
        recalls,
        primary,
        secondary,
        mrr,
        family,
        per_tool,
        persistent,
        violations,
        query_p50,
        descriptor_projection_ms,
    ))
}

fn unknown_renamed_mrr(head: &Head, cases: &[RankedCase]) -> Result<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for ranked_case in cases.iter().filter(|item| {
        item.case
            .tags
            .iter()
            .any(|tag| tag.contains("unknown") || tag.contains("renamed"))
            || item.case.case_id.contains("unknown")
            || item.case.case_id.contains("renamed")
    }) {
        let raw = ranked_case
            .descriptors
            .iter()
            .map(|(_, vector)| vector.clone())
            .collect::<Vec<_>>();
        let values = head.scores(&ranked_case.query, &raw)?;
        let mut scores = ranked_case
            .descriptors
            .iter()
            .zip(values)
            .map(|((candidate, _), score)| (candidate.name.clone(), score))
            .collect::<Vec<_>>();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for tool in ranked_case.case.relevance.keys() {
            if let Some(rank) = scores.iter().position(|(name, _)| name == tool) {
                sum += 1.0 / (rank + 1) as f64;
                count += 1;
            }
        }
    }
    Ok(if count == 0 { 0.0 } else { sum / count as f64 })
}

fn baseline_generalization(cases: &[RankedCase]) -> Result<(BTreeMap<String, f64>, f64, f64)> {
    let mut family_counts = BTreeMap::<String, (usize, usize)>::new();
    let mut unknown_rr = 0.0;
    let mut unknown_count = 0usize;
    let mut masked_relevant = 0usize;
    let mut masked_recovered = 0usize;
    let all_names = cases
        .iter()
        .flat_map(|case| {
            case.descriptors
                .iter()
                .map(|(candidate, _)| candidate.name.clone())
        })
        .collect::<BTreeSet<_>>();
    let masked_names = all_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), format!("masked_tool_{index:04}")))
        .collect::<BTreeMap<_, _>>();
    for ranked_case in cases {
        let mut scores = ranked_case
            .descriptors
            .iter()
            .map(|(candidate, vector)| (candidate.name.clone(), cosine(&ranked_case.query, vector)))
            .collect::<Vec<_>>();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for tool in ranked_case.case.relevance.keys() {
            if ranked_case.descriptors.len() == 64 {
                let found = scores.iter().take(32).any(|(name, _)| name == tool);
                let family = tool.split('_').next().unwrap_or(tool).to_string();
                let count = family_counts.entry(family).or_default();
                count.0 += 1;
                count.1 += usize::from(found);
            }
            if ranked_case
                .case
                .tags
                .iter()
                .any(|tag| tag.contains("unknown") || tag.contains("renamed"))
            {
                if let Some(rank) = scores.iter().position(|(name, _)| name == tool) {
                    unknown_rr += 1.0 / (rank + 1) as f64;
                    unknown_count += 1;
                }
            }
        }
        if ranked_case.descriptors.len() == 64 {
            let metadata = ranked_case
                .descriptors
                .iter()
                .map(|(candidate, _)| {
                    let mut candidate = candidate.clone();
                    candidate.name = masked_names[&candidate.name].clone();
                    crate::tool::catalog::ToolMetadata {
                        name: candidate.name,
                        description: candidate.description,
                        parameters: serde_json::Value::Null,
                        defer_load: true,
                        category: candidate.category,
                        disclosure: "deferred".into(),
                    }
                })
                .collect::<Vec<_>>();
            let ranked = crate::tool::catalog::ToolCatalog::rank_descriptors(
                &query_text(&ranked_case.case),
                &metadata,
                crate::tool::catalog::SearchMode::BM25,
            );
            let mut order = ranked
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>();
            let present = order.iter().cloned().collect::<BTreeSet<_>>();
            let mut missing = metadata
                .iter()
                .filter(|item| !present.contains(&item.name))
                .map(|item| item.name.clone())
                .collect::<Vec<_>>();
            missing.sort();
            order.extend(missing);
            let reverse_names = masked_names
                .iter()
                .map(|(name, masked)| (masked.clone(), name.clone()))
                .collect::<BTreeMap<_, _>>();
            for tool in ranked_case.case.relevance.keys() {
                masked_relevant += 1;
                let hit = order
                    .iter()
                    .take(32)
                    .any(|masked| reverse_names.get(masked) == Some(tool));
                masked_recovered += usize::from(hit);
            }
        }
    }
    let family = family_counts
        .into_iter()
        .map(|(name, (relevant, recovered))| (name, recovered as f64 / relevant.max(1) as f64))
        .collect();
    Ok((
        family,
        if unknown_count == 0 {
            0.0
        } else {
            unknown_rr / unknown_count as f64
        },
        masked_recovered as f64 / masked_relevant.max(1) as f64,
    ))
}

fn prepare_name_masked_cases(
    cases: &[RankedCase],
    encoder: &CandleBertSequenceEncoder,
) -> Result<Vec<RankedCase>> {
    let mut masked_names = BTreeMap::new();
    let all_names = cases
        .iter()
        .flat_map(|case| {
            case.descriptors
                .iter()
                .map(|(candidate, _)| candidate.name.clone())
        })
        .collect::<BTreeSet<_>>();
    for (index, name) in all_names.iter().enumerate() {
        masked_names.insert(name.clone(), format!("masked_tool_{index:04}"));
    }
    let mut masked_vectors: HashMap<String, Vec<f32>> = HashMap::new();
    let mut output = Vec::new();
    for ranked_case in cases.iter().filter(|case| case.descriptors.len() == 64) {
        let mut descriptors = Vec::new();
        for (candidate, _) in &ranked_case.descriptors {
            let mut masked = candidate.clone();
            masked.name = masked_names[&candidate.name].clone();
            masked.synthetic_identity = true;
            let text = descriptor_text(&masked);
            let vector = if let Some(vector) = masked_vectors.get(&text) {
                vector.clone()
            } else {
                let vector = encoder.encode_context(&text, PoolingStrategy::Mean)?;
                masked_vectors.insert(text, vector.clone());
                vector
            };
            descriptors.push((candidate.clone(), vector));
        }
        output.push(RankedCase {
            case: ranked_case.case.clone(),
            query: ranked_case.query.clone(),
            descriptors,
        });
    }
    Ok(output)
}

fn name_masked_recall(head: &Head, cases: &[RankedCase]) -> Result<f64> {
    let (recall, _, _, _, _, _, _, _, _, _) = evaluate(head, cases, &[32])?;
    Ok(recall.get("u64-k32").copied().unwrap_or(0.0))
}

fn family_parameter_count(family: &str) -> u64 {
    match family {
        "shared-linear-384x128" => (HIDDEN * PROJECTED + PROJECTED) as u64,
        "asymmetric-linear-384x128" => (2 * (HIDDEN * PROJECTED + PROJECTED)) as u64,
        "asymmetric-mlp-384x128x128" => {
            (2 * (HIDDEN * PROJECTED + PROJECTED + PROJECTED * PROJECTED + PROJECTED)) as u64
        }
        _ => 0,
    }
}

fn hard_negative_manifest(pairs: &[Pair]) -> Result<Vec<u8>> {
    let manifest = pairs
        .iter()
        .map(|pair| {
            serde_json::json!({
                "case_id": pair.case_id,
                "tool": pair.tool,
                "hard_negatives": pair.negatives.iter().map(|(name, _, same_family)| {
                    serde_json::json!({"name": name, "same_family": same_family})
                }).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_vec_pretty(&manifest)?)
}

/// Create and atomically freeze train-only negative identities without
/// training or evaluating a projection. Commit this receipt before invoking
/// [`run_m003_sweep`].
pub fn freeze_m003_hard_negatives() -> Result<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus = load_cases(None)?;
    let partition = partition_cases(&corpus);
    let (train, audit) = audited_optimizer_cases(&corpus, &partition.train_cases)?;
    if audit.optimizer_cases != 102 || train.len() != 102 {
        return Err(anyhow!(
            "M003 train optimizer input differs from frozen M011 audit"
        ));
    }
    let manifest_path = root.join(M002_ENCODER_MANIFEST);
    let encoder = CandleBertSequenceEncoder::load(&manifest_path, &Device::Cpu)?;
    let pairs = build_pairs(&train, &encoder, &mut HashMap::new())?;
    if pairs.len() != 119
        || pairs
            .iter()
            .any(|pair| pair.negatives.len() != PROJECTION_HARD_NEGATIVES_PER_POSITIVE)
    {
        return Err(anyhow!(
            "M003 hard-negative source did not produce all frozen positives and seven negatives"
        ));
    }
    let bytes = hard_negative_manifest(&pairs)?;
    let hash = digest(&bytes);
    if hash != HARD_NEGATIVE_SHA256 {
        return Err(anyhow!("M003 hard-negative fingerprint changed: {hash}"));
    }
    let path = root.join(M003_HARD_NEGATIVE_PATH);
    if path.exists() {
        if fs::read(&path)? != bytes {
            return Err(anyhow!(
                "M003 frozen hard-negative identities differ from existing receipt"
            ));
        }
    } else {
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, bytes)?;
        fs::rename(temp, path)?;
    }
    Ok(hash)
}

pub fn run_m003_sweep() -> Result<M003Receipt> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let m001_path = root.join("assets/tool-advisor/retrieval-signal-m001-preregistration.json");
    let audit_path = root.join("assets/tool-advisor/retrieval-signal-m003-train-audit.json");
    let m002_path = root.join("assets/tool-advisor/retrieval-signal-m002-frontier.json");
    let m006_path = root.join("assets/tool-advisor/retrieval-signal-m006-decision.json");
    let m001_hash = digest_file(&m001_path)?;
    let audit_hash = digest_file(&audit_path)?;
    let m002_hash = digest_file(&m002_path)?;
    let m006_hash = digest_file(&m006_path)?;
    if audit_hash != TRAIN_AUDIT_SHA256
        || m002_hash != M002_RECEIPT_SHA256
        || m006_hash != M006_DECISION_SHA256
    {
        return Err(anyhow!("M003 dependency receipt fingerprint mismatch"));
    }
    let corpus = load_cases(None)?;
    let dataset_hash = dataset_fingerprint(&corpus)?;
    let partition = partition_cases(&corpus);
    let (train, train_audit) = audited_optimizer_cases(&corpus, &partition.train_cases)?;
    if train_audit.optimizer_cases != train.len() || train_audit.optimizer_cases != 102 {
        return Err(anyhow!(
            "M003 optimizer input differs from the accepted M011 train view"
        ));
    }
    let optimizer_sha = dataset_fingerprint(&train)?;
    if optimizer_sha != train_audit.optimizer_input_sha256 {
        return Err(anyhow!("accepted optimizer view fingerprint drift"));
    }
    let (dev, dev_dataset, dev_hash) = current_step_dev_cases()?;
    if dev_dataset != dataset_hash {
        return Err(anyhow!("M003 dataset identity differs from M002"));
    }
    let device = Device::Cpu;
    let encoder_manifest_path = root.join(M002_ENCODER_MANIFEST);
    let encoder_manifest_bytes = fs::read(&encoder_manifest_path)?;
    let encoder_manifest_sha = digest(&encoder_manifest_bytes);
    if encoder_manifest_sha != ENCODER_MANIFEST_SHA256 {
        return Err(anyhow!("M003 pinned encoder manifest fingerprint changed"));
    }
    let encoder = CandleBertSequenceEncoder::load(&encoder_manifest_path, &device)?;
    let encoder_weights_sha = digest_file(&encoder.assets.weights_path)?;
    let encoder_vocabulary_sha = digest_file(&encoder.assets.vocabulary_path)?;
    let mut vectors = HashMap::new();
    let pairs = build_pairs(&train, &encoder, &mut vectors)?;
    if pairs.is_empty()
        || pairs
            .iter()
            .any(|pair| pair.negatives.len() != PROJECTION_HARD_NEGATIVES_PER_POSITIVE)
    {
        return Err(anyhow!(
            "M003 training pairs lack the preregistered seven hard negatives"
        ));
    }
    let negative_bytes = hard_negative_manifest(&pairs)?;
    let hard_negative_sha = digest(&negative_bytes);
    let negative_path = root.join(M003_HARD_NEGATIVE_PATH);
    if !negative_path.exists()
        || digest_file(&negative_path)? != HARD_NEGATIVE_SHA256
        || hard_negative_sha != HARD_NEGATIVE_SHA256
    {
        return Err(anyhow!(
            "M003 hard-negative identities must be frozen and committed before fitting"
        ));
    }
    let dev_cases = prepare_dev(&dev, &encoder, &mut vectors)?;
    let masked_dev_cases = prepare_name_masked_cases(&dev_cases, &encoder)?;
    let (baseline_families, baseline_unknown_mrr, lexical_masked_recall) =
        baseline_generalization(&dev_cases)?;
    let schema_spec = serde_json::json!({
        "schema_version": super::retrieval_signal::RETRIEVAL_SIGNAL_SCHEMA_VERSION,
        "query_field_order": super::retrieval_signal::QUERY_FIELD_ORDER,
        "query_field_byte_cap": super::retrieval_signal::QUERY_FIELD_BYTE_CAP,
        "query_total_byte_cap": super::retrieval_signal::QUERY_TOTAL_BYTE_CAP,
        "descriptor_field_order": super::retrieval_signal::DESCRIPTOR_FIELD_ORDER,
        "descriptor_schema_byte_cap": super::retrieval_signal::DESCRIPTOR_SCHEMA_BYTE_CAP,
        "descriptor_schema_field_desc_cap": super::retrieval_signal::DESCRIPTOR_SCHEMA_FIELD_DESC_CAP,
        "normalization": ["NFKC", "unicode-lowercase", "identifier-decomposition", "punctuation-separation", "no-synonyms"]
    });
    let representation_schema_sha256 = digest(&serde_json::to_vec(&schema_spec)?);
    let mut grid_points = Vec::new();
    let mut best: Option<(usize, (usize, usize, u64, usize))> = None;
    let mut best_weights: Option<Vec<u8>> = None;
    for family in PROJECTION_FAMILIES {
        for learning_rate in PROJECTION_LR_GRID {
            for temperature in PROJECTION_TEMPERATURE_GRID {
                for seed in PROJECTION_SEEDS {
                    let head = Head::new(family, seed, &device)?;
                    let parameters = head.parameter_count();
                    if parameters > MAX_PROJECTION_PARAMS
                        || parameters != family_parameter_count(family)
                    {
                        return Err(anyhow!("M003 projection parameter contract violated for {family}: {parameters}"));
                    }
                    let losses = train_head(&head, &pairs, learning_rate, temperature, seed)?;
                    let (
                        recall,
                        primary_recall,
                        secondary_recall,
                        mrr,
                        family_recall,
                        per_tool_recall_u64_k32,
                        persistent_miss_recall_u64_k32,
                        authority_violations,
                        query_projection_latency_p50_ms,
                        descriptor_projection_cache_ms,
                    ) = evaluate(&head, &dev_cases, &PREREG_PRIMARY_KS)?;
                    let unknown_mrr = unknown_renamed_mrr(&head, &dev_cases)?;
                    let masked_recall = name_masked_recall(&head, &masked_dev_cases)?;
                    let family_guard = family_recall.iter().all(|(family, recall)| {
                        *recall + 0.03 >= baseline_families.get(family).copied().unwrap_or(0.0)
                    });
                    let generalization_pass = unknown_mrr + 0.02 >= baseline_unknown_mrr
                        && family_guard
                        && masked_recall > lexical_masked_recall;
                    let selected_k = [16usize, 24, 32].into_iter().find(|k| {
                        recall.get(&format!("u64-k{k}")).copied().unwrap_or(0.0) >= GATE_RECALL_64
                            && recall.get(&format!("u128-k{k}")).copied().unwrap_or(0.0)
                                >= GATE_RECALL_128
                            && recall.get(&format!("u256-k{k}")).copied().unwrap_or(0.0)
                                >= GATE_RECALL_256
                    });
                    let point_index = grid_points.len();
                    let meets =
                        selected_k.is_some() && authority_violations == 0 && generalization_pass;
                    grid_points.push(M003Point {
                        family: family.into(),
                        learning_rate,
                        temperature,
                        seed,
                        epochs: PROJECTION_MAX_EPOCHS,
                        parameters,
                        train_loss: losses,
                        recall,
                        primary_recall,
                        secondary_recall,
                        mrr,
                        per_tool_recall_u64_k32,
                        persistent_miss_recall_u64_k32,
                        family_recall,
                        unknown_renamed_mrr: unknown_mrr,
                        name_masked_recall_u64_k32: masked_recall,
                        authority_violations,
                        generalization_pass,
                        selected_k,
                        query_projection_latency_p50_ms,
                        descriptor_projection_cache_ms,
                    });
                    if meets
                        && best.as_ref().is_none_or(|(_, key)| {
                            (
                                parameters as usize,
                                selected_k.unwrap_or(32),
                                ((query_projection_latency_p50_ms + descriptor_projection_cache_ms)
                                    * 1000.0) as u64,
                                point_index,
                            ) < *key
                        })
                    {
                        let temp_weights = root
                            .join(M003_OUTPUT_DIR)
                            .join("selected-projection.tmp.safetensors");
                        fs::create_dir_all(
                            temp_weights.parent().expect("projection artifact parent"),
                        )?;
                        head.varmap.save(&temp_weights)?;
                        let weights = fs::read(&temp_weights)?;
                        let _ = fs::remove_file(&temp_weights);
                        best = Some((
                            point_index,
                            (
                                parameters as usize,
                                selected_k.unwrap_or(32),
                                ((query_projection_latency_p50_ms + descriptor_projection_cache_ms)
                                    * 1000.0) as u64,
                                point_index,
                            ),
                        ));
                        best_weights = Some(weights);
                    }
                }
            }
        }
    }
    let selected_point = best.map(|(index, _)| index);
    let disposition = if selected_point.is_some() {
        "positive: preregistered projection clears all recall, authority, and name-mask gates"
            .to_string()
    } else {
        "negative-valid: no preregistered projection clears the frozen recall and authority gates"
            .to_string()
    };
    let mut artifact_sha = None;
    let mut artifact_bytes = None;
    if let Some(bytes) = best_weights {
        let path = root
            .join(M003_OUTPUT_DIR)
            .join("selected-projection.safetensors");
        fs::create_dir_all(path.parent().expect("projection artifact parent"))?;
        fs::write(&path, &bytes)?;
        artifact_sha = Some(digest(&bytes));
        artifact_bytes = Some(bytes.len() as u64);
    }
    let receipt = M003Receipt {
        schema_version: 1,
        protocol: M003_PROTOCOL.into(),
        m001_preregistration_sha256: m001_hash,
        m002_receipt_sha256: m002_hash,
        m006_decision_sha256: m006_hash,
        train_audit_sha256: audit_hash,
        dataset_sha256: dataset_hash,
        train_partition_sha256: train_audit.train_partition_sha256,
        dev_partition_sha256: dev_hash,
        encoder_manifest_sha256: encoder_manifest_sha,
        encoder_weights_sha256: encoder_weights_sha,
        encoder_vocabulary_sha256: encoder_vocabulary_sha,
        representation_schema_sha256,
        projection_contract_sha256: projection_contract_sha256()?,
        baseline_family_recall_u64_k32: baseline_families,
        baseline_unknown_renamed_mrr: baseline_unknown_mrr,
        name_masked_lexical_baseline_recall_u64_k32: lexical_masked_recall,
        optimizer_input_sha256: optimizer_sha,
        hard_negative_sha256: hard_negative_sha,
        cases_train: train.len(),
        cases_dev: dev.len(),
        dev_no_tool_cases: dev.iter().filter(|case| case.relevance.is_empty()).count(),
        grid_points,
        selected_point,
        disposition,
        selected_artifact_sha256: artifact_sha,
        selected_artifact_bytes: artifact_bytes,
        incremental_rss_bytes: None,
    };
    let path = root.join(M003_RECEIPT_PATH);
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(head: &Head) -> BTreeMap<String, Vec<f32>> {
        head.varmap
            .data()
            .lock()
            .expect("head varmap")
            .iter()
            .map(|(name, var)| {
                (
                    name.clone(),
                    var.as_tensor()
                        .flatten_all()
                        .expect("flatten")
                        .to_vec1::<f32>()
                        .expect("values"),
                )
            })
            .collect()
    }

    #[test]
    fn projection_families_obey_parameter_cap_and_seed_deterministically() {
        let device = Device::Cpu;
        for family in PROJECTION_FAMILIES {
            let first = Head::new(family, 21, &device).expect("first head");
            let second = Head::new(family, 21, &device).expect("same-seed head");
            assert_eq!(first.parameter_count(), family_parameter_count(family));
            assert!(first.parameter_count() <= MAX_PROJECTION_PARAMS);
            assert_eq!(snapshot(&first), snapshot(&second));
            let different_seed = Head::new(family, 42, &device).expect("different-seed head");
            assert_ne!(snapshot(&first), snapshot(&different_seed));
        }
    }

    #[test]
    fn receipt_identity_names_the_frozen_grid() {
        assert_eq!(
            PROJECTION_FAMILIES.len()
                * PROJECTION_LR_GRID.len()
                * PROJECTION_TEMPERATURE_GRID.len()
                * PROJECTION_SEEDS.len(),
            81
        );
        assert_eq!(PROJECTION_MAX_EPOCHS, 5);
        assert_eq!(PROJECTION_HARD_NEGATIVES_PER_POSITIVE, 7);
        assert_eq!(
            projection_contract_sha256().expect("contract hash").len(),
            64
        );
        assert_eq!(HARD_NEGATIVE_SHA256.len(), 64);
    }

    #[test]
    #[ignore = "rebuilds and verifies the frozen M003 train-only hard-negative receipt"]
    fn m003_hard_negative_receipt_freezes_before_training() {
        assert_eq!(
            freeze_m003_hard_negatives().expect("frozen negatives"),
            HARD_NEGATIVE_SHA256
        );
    }

    #[test]
    #[ignore = "full preregistered M003 train/dev projection sweep"]
    fn m003_full_projection_sweep_emits_receipt() {
        let receipt = run_m003_sweep().expect("M003 frozen projection sweep");
        assert_eq!(receipt.grid_points.len(), 81);
        assert_eq!(
            receipt.disposition.starts_with("positive:"),
            receipt.selected_point.is_some()
        );
    }
}
