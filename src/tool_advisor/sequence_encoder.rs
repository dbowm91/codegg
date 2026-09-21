//! Pure-Rust, local-asset BERT sequence-encoder experiment support.
//!
//! This module is deliberately feature-gated and has no network or model
//! download path. It owns only the experiment contract: verified local
//! assets, deterministic WordPiece input construction, Candle BERT loading,
//! and staged optimizer parameter selection. It is not a production advisor
//! and does not receive policy or execution authority.

use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor, Var};
use candle_nn::{AdamW, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap};
use candle_transformers::models::bert::{BertModel, Config, HiddenAct};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const ASSET_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const ENCODER_ARCHITECTURE: &str = "bert-sequence-encoder-candle-v1";
pub const MAX_PAIR_TOKENS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetManifest {
    pub schema_version: u16,
    pub architecture: String,
    pub framework: String,
    pub config_path: String,
    pub vocabulary_path: String,
    pub weights_path: String,
    pub license_path: String,
    pub hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssets {
    pub manifest: AssetManifest,
    pub manifest_path: PathBuf,
    pub config_path: PathBuf,
    pub vocabulary_path: PathBuf,
    pub weights_path: PathBuf,
    pub license_path: PathBuf,
}

impl ResolvedAssets {
    pub fn load(manifest_path: &Path) -> Result<Self> {
        let bytes = fs::read(manifest_path)
            .with_context(|| format!("read encoder asset manifest {}", manifest_path.display()))?;
        let manifest: AssetManifest =
            serde_json::from_slice(&bytes).context("parse encoder asset manifest")?;
        if manifest.schema_version != ASSET_MANIFEST_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported encoder asset manifest schema {}",
                manifest.schema_version
            ));
        }
        if manifest.architecture != ENCODER_ARCHITECTURE || manifest.framework != "candle-0.11" {
            return Err(anyhow!(
                "unsupported encoder asset contract: {}/{}",
                manifest.architecture,
                manifest.framework
            ));
        }
        let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let resolve = |relative: &str| base.join(relative);
        let assets = Self {
            config_path: resolve(&manifest.config_path),
            vocabulary_path: resolve(&manifest.vocabulary_path),
            weights_path: resolve(&manifest.weights_path),
            license_path: resolve(&manifest.license_path),
            manifest,
            manifest_path: manifest_path.to_path_buf(),
        };
        for (label, path) in [
            ("config", &assets.config_path),
            ("vocabulary", &assets.vocabulary_path),
            ("weights", &assets.weights_path),
            ("license", &assets.license_path),
        ] {
            let expected = assets
                .manifest
                .hashes
                .get(label)
                .ok_or_else(|| anyhow!("manifest is missing {label} hash"))?;
            let actual = sha256_file(path)?;
            if &actual != expected {
                return Err(anyhow!(
                    "encoder {label} hash mismatch: expected {expected}, found {actual}"
                ));
            }
        }
        Ok(assets)
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read asset {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[derive(Debug, Clone)]
pub struct WordPieceTokenizer {
    ids: BTreeMap<String, u32>,
    unknown_id: u32,
    cls_id: u32,
    sep_id: u32,
}

impl WordPieceTokenizer {
    pub fn from_vocab(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read WordPiece vocabulary {}", path.display()))?;
        let ids = text
            .lines()
            .enumerate()
            .filter_map(|(index, token)| {
                let token = token.trim();
                (!token.is_empty()).then(|| (token.to_string(), index as u32))
            })
            .collect::<BTreeMap<_, _>>();
        let id = |token: &str| {
            ids.get(token)
                .copied()
                .ok_or_else(|| anyhow!("vocabulary is missing {token}"))
        };
        Ok(Self {
            unknown_id: id("[UNK]")?,
            cls_id: id("[CLS]")?,
            sep_id: id("[SEP]")?,
            ids,
        })
    }

    pub fn encode_pair(&self, context: &str, candidate: &str, max_tokens: usize) -> EncodedPair {
        let budget = max_tokens.max(4);
        let mut context_tokens = self.tokenize(context);
        let mut candidate_tokens = self.tokenize(candidate);
        while context_tokens.len() + candidate_tokens.len() + 3 > budget {
            if context_tokens.len() >= candidate_tokens.len() && !context_tokens.is_empty() {
                context_tokens.pop();
            } else if !candidate_tokens.is_empty() {
                candidate_tokens.pop();
            } else {
                break;
            }
        }
        let mut input_ids = vec![self.cls_id];
        let mut token_type_ids = vec![0u32];
        input_ids.extend(&context_tokens);
        token_type_ids.extend(std::iter::repeat_n(0, context_tokens.len()));
        input_ids.push(self.sep_id);
        token_type_ids.push(0);
        input_ids.extend(&candidate_tokens);
        token_type_ids.extend(std::iter::repeat_n(1, candidate_tokens.len()));
        input_ids.push(self.sep_id);
        token_type_ids.push(1);
        let attention_mask = vec![1u32; input_ids.len()];
        EncodedPair {
            input_ids,
            token_type_ids,
            attention_mask,
        }
    }

    fn tokenize(&self, text: &str) -> Vec<u32> {
        text.split_whitespace()
            .flat_map(|word| self.wordpiece(&word.to_lowercase()))
            .collect()
    }

    fn wordpiece(&self, word: &str) -> Vec<u32> {
        if let Some(id) = self.ids.get(word) {
            return vec![*id];
        }
        let chars = word.char_indices().collect::<Vec<_>>();
        let mut out = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = chars.len();
            let mut found = None;
            while end > start {
                let begin = chars[start].0;
                let finish = if end == chars.len() {
                    word.len()
                } else {
                    chars[end].0
                };
                let fragment = &word[begin..finish];
                let key = if start == 0 {
                    fragment.to_string()
                } else {
                    format!("##{fragment}")
                };
                if let Some(id) = self.ids.get(&key) {
                    found = Some(*id);
                    break;
                }
                end -= 1;
            }
            let Some(id) = found else {
                return vec![self.unknown_id];
            };
            out.push(id);
            start = end;
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPair {
    pub input_ids: Vec<u32>,
    pub token_type_ids: Vec<u32>,
    pub attention_mask: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FineTuneStage {
    HeadOnly,
    TopLayers(usize),
    Full,
}

/// Explicit pooling strategy for pair representations.
///
/// `Cls` returns the first-token hidden state (the M001 default). `Mean`
/// returns the attention-mask-aware mean over token hidden states. M001A
/// characterizes both on a train/dev-only sanity set; M003 selects the
/// strategy explicitly per run instead of inheriting a hard-coded default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolingStrategy {
    Cls,
    Mean,
}

/// Pinned M001A reference-checkpoint facts for
/// `sentence-transformers/all-MiniLM-L6-v2` at the immutable upstream
/// revision named by the implementation plan.
pub const REFERENCE_MODEL_TYPE: &str = "bert";
pub const REFERENCE_HIDDEN_SIZE: usize = 384;
pub const REFERENCE_NUM_LAYERS: usize = 6;
pub const REFERENCE_NUM_HEADS: usize = 12;
pub const REFERENCE_VOCAB_SIZE: usize = 30_522;
pub const REFERENCE_TYPE_VOCAB_SIZE: usize = 2;
pub const REFERENCE_MAX_POSITION_EMBEDDINGS: usize = 512;

/// Validate a parsed BERT config against the pinned reference contract.
///
/// This runs before Candle tensor construction on the real-checkpoint path
/// and fails closed on any mismatch. Successful deserialization of
/// `hidden_act` into the Candle enum is itself the activation-support
/// check; the exhaustive match below keeps that contract explicit so a
/// future upstream variant cannot slip through silently.
pub fn validate_reference_checkpoint_config(config: &Config) -> Result<()> {
    let model_type = config.model_type.as_deref().unwrap_or("<missing>");
    if model_type != REFERENCE_MODEL_TYPE {
        return Err(anyhow!(
            "reference model_type is {model_type:?}; expected {REFERENCE_MODEL_TYPE:?}"
        ));
    }
    if config.hidden_size != REFERENCE_HIDDEN_SIZE {
        return Err(anyhow!(
            "reference hidden_size is {}; expected {REFERENCE_HIDDEN_SIZE}",
            config.hidden_size
        ));
    }
    if config.num_hidden_layers != REFERENCE_NUM_LAYERS {
        return Err(anyhow!(
            "reference num_hidden_layers is {}; expected {REFERENCE_NUM_LAYERS}",
            config.num_hidden_layers
        ));
    }
    if config.num_attention_heads != REFERENCE_NUM_HEADS {
        return Err(anyhow!(
            "reference num_attention_heads is {}; expected {REFERENCE_NUM_HEADS}",
            config.num_attention_heads
        ));
    }
    if config.vocab_size != REFERENCE_VOCAB_SIZE {
        return Err(anyhow!(
            "reference vocab_size is {}; expected {REFERENCE_VOCAB_SIZE}",
            config.vocab_size
        ));
    }
    if config.type_vocab_size != REFERENCE_TYPE_VOCAB_SIZE {
        return Err(anyhow!(
            "reference type_vocab_size is {}; expected {REFERENCE_TYPE_VOCAB_SIZE}",
            config.type_vocab_size
        ));
    }
    if config.max_position_embeddings != REFERENCE_MAX_POSITION_EMBEDDINGS {
        return Err(anyhow!(
            "reference max_position_embeddings is {}; expected \
             {REFERENCE_MAX_POSITION_EMBEDDINGS}",
            config.max_position_embeddings
        ));
    }
    if !config
        .hidden_size
        .is_multiple_of(config.num_attention_heads)
    {
        return Err(anyhow!(
            "reference hidden size is not divisible into attention heads"
        ));
    }
    match config.hidden_act {
        HiddenAct::Gelu | HiddenAct::GeluApproximate | HiddenAct::Relu => {}
    }
    Ok(())
}

/// Load-coverage evidence comparing the Candle model variables against the
/// source safetensors header.
///
/// Positive M001A closure requires zero missing encoder variables.
/// Unexpected source tensors (pooler head, position ids) are reported but
/// do not fail the load because `VarMap::load` ignores tensors that have
/// no corresponding model variable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadCoverageReport {
    pub expected_variables: usize,
    pub loaded_variables: usize,
    pub missing_variables: Vec<String>,
    pub unexpected_source_tensors: Vec<String>,
    pub loaded_parameter_count: u64,
    pub source_parameter_count: u64,
}

pub fn load_coverage(weights_path: &Path, varmap: &VarMap) -> Result<LoadCoverageReport> {
    let bytes = fs::read(weights_path)
        .with_context(|| format!("read safetensors {}", weights_path.display()))?;
    if bytes.len() < 8 {
        return Err(anyhow!("safetensors file is truncated"));
    }
    let header_len =
        u64::from_le_bytes(bytes[0..8].try_into().expect("header length prefix")) as usize;
    if bytes.len() < 8 + header_len {
        return Err(anyhow!("safetensors header is truncated"));
    }
    let header: serde_json::Value =
        serde_json::from_slice(&bytes[8..8 + header_len]).context("parse safetensors header")?;
    let object = header
        .as_object()
        .ok_or_else(|| anyhow!("safetensors header is not an object"))?;
    let mut source_shapes: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (name, descriptor) in object {
        if name == "__metadata__" {
            continue;
        }
        let shape = descriptor
            .get("shape")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("safetensors entry {name} has no shape"))?;
        let mut dims = Vec::with_capacity(shape.len());
        for dim in shape {
            let dim = dim
                .as_u64()
                .ok_or_else(|| anyhow!("safetensors entry {name} has a non-integer shape"))?
                as usize;
            dims.push(dim);
        }
        source_shapes.insert(name.clone(), dims);
    }
    let data = varmap.data().lock().expect("varmap lock");
    let mut missing = Vec::new();
    let mut loaded = 0usize;
    let mut loaded_params = 0u64;
    for (name, var) in data.iter() {
        match source_shapes.get(name) {
            Some(shape) if var.as_tensor().dims() == shape.as_slice() => {
                loaded += 1;
                loaded_params += var.as_tensor().elem_count() as u64;
            }
            Some(shape) => missing.push(format!(
                "shape mismatch for {name}: model expects {:?}, source has {shape:?}",
                var.as_tensor().dims()
            )),
            None => missing.push(name.clone()),
        }
    }
    missing.sort();
    let mut unexpected: Vec<String> = source_shapes
        .keys()
        .filter(|name| !data.contains_key(*name))
        .cloned()
        .collect();
    unexpected.sort();
    let source_params = source_shapes
        .values()
        .map(|shape| {
            shape
                .iter()
                .fold(1u64, |count, dim| count.saturating_mul(*dim as u64))
        })
        .sum();
    Ok(LoadCoverageReport {
        expected_variables: data.len(),
        loaded_variables: loaded,
        missing_variables: missing,
        unexpected_source_tensors: unexpected,
        loaded_parameter_count: loaded_params,
        source_parameter_count: source_params,
    })
}

pub struct CandleBertSequenceEncoder {
    pub assets: ResolvedAssets,
    pub config: Config,
    pub tokenizer: WordPieceTokenizer,
    pub device: Device,
    pub varmap: VarMap,
    model: BertModel,
}

impl CandleBertSequenceEncoder {
    pub fn load(manifest_path: &Path, device: &Device) -> Result<Self> {
        let assets = ResolvedAssets::load(manifest_path)?;
        let config: Config =
            serde_json::from_slice(&fs::read(&assets.config_path).context("read BERT config")?)
                .context("parse BERT config")?;
        let tokenizer = WordPieceTokenizer::from_vocab(&assets.vocabulary_path)?;
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, device);
        let model = BertModel::load(vb, &config).context("construct BERT model")?;
        varmap
            .clone()
            .load(&assets.weights_path)
            .context("load local BERT safetensors")?;
        Ok(Self {
            assets,
            config,
            tokenizer,
            device: device.clone(),
            varmap,
            model,
        })
    }

    pub fn encode(&self, context: &str, candidate: &str) -> Result<Vec<f32>> {
        self.encode_with_pooling(context, candidate, PoolingStrategy::Cls)
    }

    pub fn encode_with_pooling(
        &self,
        context: &str,
        candidate: &str,
        strategy: PoolingStrategy,
    ) -> Result<Vec<f32>> {
        let (sequence, mask) = self.forward_sequence(context, candidate)?;
        match strategy {
            PoolingStrategy::Cls => first_token_vector(&sequence),
            PoolingStrategy::Mean => mean_pooled_vector(&sequence, &mask),
        }
    }

    pub fn encode_mean_pooled(&self, context: &str, candidate: &str) -> Result<Vec<f32>> {
        self.encode_with_pooling(context, candidate, PoolingStrategy::Mean)
    }

    /// Compare first-token and mean pooling on a fixed train/dev-only
    /// sanity set. The obvious related/unrelated descriptor pairs below
    /// are hand-written so the probe never inspects frozen final-test or
    /// family-holdout labels. The report records cosine separation for
    /// both strategies; M003 selects the strategy explicitly per run.
    pub fn pooling_separation(&self) -> Result<PoolingSeparationReport> {
        let context = "find files that mention TODO in the repository";
        let paraphrase = "locate repository files containing TODO markers";
        let related = "search file contents across the project with grep";
        let unrelated = "send an invoice email to the customer";
        let cls_base = self.encode_with_pooling(context, related, PoolingStrategy::Cls)?;
        let cls_related = self.encode_with_pooling(paraphrase, related, PoolingStrategy::Cls)?;
        let cls_unrelated = self.encode_with_pooling(context, unrelated, PoolingStrategy::Cls)?;
        let mean_base = self.encode_with_pooling(context, related, PoolingStrategy::Mean)?;
        let mean_related = self.encode_with_pooling(paraphrase, related, PoolingStrategy::Mean)?;
        let mean_unrelated = self.encode_with_pooling(context, unrelated, PoolingStrategy::Mean)?;
        let cls_related_cosine = cosine_similarity(&cls_base, &cls_related);
        let cls_unrelated_cosine = cosine_similarity(&cls_base, &cls_unrelated);
        let mean_related_cosine = cosine_similarity(&mean_base, &mean_related);
        let mean_unrelated_cosine = cosine_similarity(&mean_base, &mean_unrelated);
        let vectors = [
            &cls_base,
            &cls_related,
            &cls_unrelated,
            &mean_base,
            &mean_related,
            &mean_unrelated,
        ];
        let all_finite = vectors
            .iter()
            .all(|vector| vector.iter().all(|value| value.is_finite()))
            && cls_related_cosine.is_finite()
            && cls_unrelated_cosine.is_finite()
            && mean_related_cosine.is_finite()
            && mean_unrelated_cosine.is_finite();
        Ok(PoolingSeparationReport {
            cls_related_cosine,
            cls_unrelated_cosine,
            cls_margin: cls_related_cosine - cls_unrelated_cosine,
            mean_related_cosine,
            mean_unrelated_cosine,
            mean_margin: mean_related_cosine - mean_unrelated_cosine,
            all_finite_and_distinct: all_finite
                && cls_related_cosine != cls_unrelated_cosine
                && mean_related_cosine != mean_unrelated_cosine,
        })
    }

    fn forward_sequence(&self, context: &str, candidate: &str) -> Result<(Tensor, Vec<u32>)> {
        let encoded = self
            .tokenizer
            .encode_pair(context, candidate, MAX_PAIR_TOKENS);
        let input_ids = Tensor::new(encoded.input_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let token_type_ids =
            Tensor::new(encoded.token_type_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let mask = encoded.attention_mask.clone();
        let attention_mask =
            Tensor::new(encoded.attention_mask.as_slice(), &self.device)?.unsqueeze(0)?;
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;
        Ok((hidden, mask))
    }

    pub fn top_layer_vars(&self, count: usize) -> Vec<Var> {
        let first_layer = self.config.num_hidden_layers.saturating_sub(count);
        self.varmap
            .data()
            .lock()
            .expect("varmap lock")
            .iter()
            .filter(|(name, _)| {
                (first_layer..self.config.num_hidden_layers)
                    .any(|layer| name.contains(&format!("encoder.layer.{layer}.")))
            })
            .map(|(_, var)| var.clone())
            .collect()
    }

    pub fn optimizer_for_stage(
        &self,
        head: &ScalarRankingHead,
        stage: FineTuneStage,
        learning_rate: f64,
    ) -> Result<AdamW> {
        let mut vars = head.varmap.all_vars();
        match stage {
            FineTuneStage::HeadOnly => {}
            FineTuneStage::TopLayers(count) => vars.extend(self.top_layer_vars(count.max(1))),
            FineTuneStage::Full => vars.extend(self.varmap.all_vars()),
        }
        Ok(AdamW::new(
            vars,
            ParamsAdamW {
                lr: learning_rate,
                weight_decay: 0.0,
                ..Default::default()
            },
        )?)
    }

    pub fn train_step(
        &self,
        head: &ScalarRankingHead,
        optimizer: &mut AdamW,
        positive: (&str, &str),
        negative: (&str, &str),
    ) -> Result<f32> {
        let positive = head.score(&self.encode_tensor(positive.0, positive.1)?)?;
        let negative = head.score(&self.encode_tensor(negative.0, negative.1)?)?;
        let margin = Tensor::new(&[[1f32]], &self.device)?;
        let loss = (margin - (positive - negative)?)?.relu()?.mean_all()?;
        let value = loss.to_vec0::<f32>()?;
        optimizer.backward_step(&loss)?;
        Ok(value)
    }

    fn encode_tensor(&self, context: &str, candidate: &str) -> Result<Tensor> {
        let encoded = self
            .tokenizer
            .encode_pair(context, candidate, MAX_PAIR_TOKENS);
        let input_ids = Tensor::new(encoded.input_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let token_type_ids =
            Tensor::new(encoded.token_type_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let attention_mask =
            Tensor::new(encoded.attention_mask.as_slice(), &self.device)?.unsqueeze(0)?;
        Ok(self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?
            .narrow(1, 0, 1)?
            .squeeze(1)?)
    }
}

/// Outcome of checking the loaded config against the pinned M001A
/// reference contract. Informational inside the generic probe: the tiny
/// fixture correctly reports no match, while the real-checkpoint probe
/// must report a match for positive closure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceConfigCheck {
    pub matches_pinned_contract: bool,
    pub detail: String,
}

fn check_reference_config(config: &Config) -> ReferenceConfigCheck {
    match validate_reference_checkpoint_config(config) {
        Ok(()) => ReferenceConfigCheck {
            matches_pinned_contract: true,
            detail: "matches the pinned all-MiniLM-L6-v2 reference contract".into(),
        },
        Err(error) => ReferenceConfigCheck {
            matches_pinned_contract: false,
            detail: error.to_string(),
        },
    }
}

fn first_token_vector(sequence: &Tensor) -> Result<Vec<f32>> {
    Ok(sequence
        .narrow(1, 0, 1)?
        .squeeze(1)?
        .squeeze(0)?
        .to_dtype(DType::F32)?
        .to_vec1()?)
}

fn mean_pooled_vector(sequence: &Tensor, mask: &[u32]) -> Result<Vec<f32>> {
    let rows = sequence
        .squeeze(0)?
        .to_dtype(DType::F32)?
        .to_vec2::<f32>()?;
    if rows.len() != mask.len() {
        return Err(anyhow!(
            "pooling mask length does not match sequence length"
        ));
    }
    let width = rows.first().map(Vec::len).unwrap_or(0);
    let mut mean = vec![0.0f32; width];
    let mut count = 0.0f32;
    for (row, keep) in rows.iter().zip(mask.iter()) {
        if *keep == 0 {
            continue;
        }
        for (acc, value) in mean.iter_mut().zip(row.iter()) {
            *acc += *value;
        }
        count += 1.0;
    }
    if count <= 0.0 {
        return Err(anyhow!("mean pooling saw an empty attention mask"));
    }
    for acc in mean.iter_mut() {
        *acc /= count;
    }
    Ok(mean)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;
    for (first, second) in left.iter().zip(right.iter()) {
        dot += first * second;
        left_norm += first * first;
        right_norm += second * second;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator.abs() < f32::EPSILON {
        0.0
    } else {
        dot / denominator
    }
}

fn snapshot_varmap(varmap: &VarMap) -> Result<BTreeMap<String, Vec<f32>>> {
    let data = varmap.data().lock().expect("varmap lock");
    let mut snapshot = BTreeMap::new();
    for (name, var) in data.iter() {
        let values = var
            .as_tensor()
            .flatten_all()?
            .to_dtype(DType::F32)?
            .to_vec1::<f32>()?;
        snapshot.insert(name.clone(), values);
    }
    Ok(snapshot)
}

/// Cosine separation between obvious related/unrelated descriptor pairs
/// for one pooling strategy comparison. M001A records both strategies;
/// beating a baseline is M003 work, not a probe gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolingSeparationReport {
    pub cls_related_cosine: f32,
    pub cls_unrelated_cosine: f32,
    pub cls_margin: f32,
    pub mean_related_cosine: f32,
    pub mean_unrelated_cosine: f32,
    pub mean_margin: f32,
    pub all_finite_and_distinct: bool,
}

pub struct ScalarRankingHead {
    pub varmap: VarMap,
    linear: candle_nn::Linear,
}

impl ScalarRankingHead {
    pub fn new(hidden_size: usize, device: &Device) -> Result<Self> {
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, device);
        let linear = candle_nn::linear(hidden_size, 1, vb.pp("ranking"))?;
        Ok(Self { varmap, linear })
    }

    fn score(&self, embedding: &Tensor) -> Result<Tensor> {
        Ok(self.linear.forward(embedding)?)
    }
}

/// Return a concise deterministic probe suitable for closure evidence.
///
/// Beyond the M001 forward/backward checks this also records the
/// safetensors load coverage (zero missing encoder variables required),
/// staged-optimizer scoping facts (which variable sets each training stage
/// actually changed), CLS/mean pooling separation on a fixed train/dev-only
/// sanity set, and wall-clock timings plus asset sizes for the resource
/// evidence. The probe reports facts; stage verdicts belong to the closure
/// record, because Candle 0.11 cannot train encoder weights (see the
/// `top_layer_changed` field documentation).
pub fn probe_local_assets(manifest_path: &Path, device: &Device) -> Result<ProbeReport> {
    let load_started = Instant::now();
    let encoder = CandleBertSequenceEncoder::load(manifest_path, device)?;
    let load_ms = load_started.elapsed().as_millis();
    let coverage = load_coverage(&encoder.assets.weights_path, &encoder.varmap)?;
    if !coverage.missing_variables.is_empty() {
        return Err(anyhow!(
            "encoder load is missing {} variables: {}",
            coverage.missing_variables.len(),
            coverage.missing_variables.join(", ")
        ));
    }
    let forward_started = Instant::now();
    let first = encoder.encode("inspect the project", "read repository files")?;
    let second = encoder.encode("inspect the project", "read repository files")?;
    let forward_ms = forward_started.elapsed().as_millis();
    let max_delta = first
        .iter()
        .zip(second.iter())
        .map(|(left, right)| (left - right).abs())
        .fold(0.0f32, f32::max);
    let embeddings_finite =
        first.iter().all(|value| value.is_finite()) && second.iter().all(|v| v.is_finite());
    let head = ScalarRankingHead::new(encoder.config.hidden_size, device)?;
    let encoder_before_head = snapshot_varmap(&encoder.varmap)?;
    let head_before = snapshot_varmap(&head.varmap)?;
    let head_started = Instant::now();
    let mut optimizer = encoder.optimizer_for_stage(&head, FineTuneStage::HeadOnly, 0.01)?;
    let head_loss = encoder.train_step(
        &head,
        &mut optimizer,
        ("inspect", "read files"),
        ("inspect", "send email"),
    )?;
    let head_step_ms = head_started.elapsed().as_millis();
    let head_only_encoder_unchanged = snapshot_varmap(&encoder.varmap)? == encoder_before_head;
    let head_vars_changed = snapshot_varmap(&head.varmap)? != head_before;
    let top_layer = encoder.config.num_hidden_layers.saturating_sub(1);
    let top_marker = format!("encoder.layer.{top_layer}.");
    let encoder_before_top = snapshot_varmap(&encoder.varmap)?;
    let head_before_top = snapshot_varmap(&head.varmap)?;
    let top_started = Instant::now();
    let mut top_optimizer =
        encoder.optimizer_for_stage(&head, FineTuneStage::TopLayers(1), 0.001)?;
    let top_layer_loss = encoder.train_step(
        &head,
        &mut top_optimizer,
        ("inspect", "read files"),
        ("inspect", "send email"),
    )?;
    let top_layer_step_ms = top_started.elapsed().as_millis();
    let encoder_after_top = snapshot_varmap(&encoder.varmap)?;
    let lower_layers_unchanged = encoder_after_top
        .iter()
        .filter(|(name, _)| !name.contains(&top_marker))
        .all(|(name, values)| encoder_before_top.get(name.as_str()) == Some(values));
    let top_layer_changed = encoder_after_top
        .iter()
        .filter(|(name, _)| name.contains(&top_marker))
        .any(|(name, values)| encoder_before_top.get(name.as_str()) != Some(values));
    let head_changed_in_top_stage = snapshot_varmap(&head.varmap)? != head_before_top;
    if !head_loss.is_finite() || !top_layer_loss.is_finite() {
        return Err(anyhow!("probe training losses must be finite"));
    }
    let pooling = encoder.pooling_separation()?;
    let reference_config = check_reference_config(&encoder.config);
    let weights_bytes = fs::metadata(&encoder.assets.weights_path)
        .with_context(|| "stat encoder weights")?
        .len();
    Ok(ProbeReport {
        architecture: ENCODER_ARCHITECTURE.into(),
        hidden_size: encoder.config.hidden_size,
        layers: encoder.config.num_hidden_layers,
        vocab_size: encoder.config.vocab_size,
        deterministic_max_delta: max_delta,
        head_only_loss: head_loss,
        top_layer_loss,
        parameter_count: coverage.loaded_parameter_count,
        weight_sha256: encoder
            .assets
            .manifest
            .hashes
            .get("weights")
            .cloned()
            .unwrap_or_default(),
        load_coverage: coverage,
        reference_config,
        embeddings_finite,
        head_only_encoder_unchanged,
        head_vars_changed,
        top_layer_changed,
        head_changed_in_top_stage,
        lower_layers_unchanged,
        pooling,
        load_ms,
        forward_ms,
        head_step_ms,
        top_layer_step_ms,
        weights_bytes,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeReport {
    pub architecture: String,
    pub hidden_size: usize,
    pub layers: usize,
    pub vocab_size: usize,
    pub deterministic_max_delta: f32,
    pub head_only_loss: f32,
    pub top_layer_loss: f32,
    pub parameter_count: u64,
    pub weight_sha256: String,
    pub load_coverage: LoadCoverageReport,
    pub reference_config: ReferenceConfigCheck,
    pub embeddings_finite: bool,
    pub head_only_encoder_unchanged: bool,
    pub head_vars_changed: bool,
    /// Whether the top-layer optimizer step changed top-layer variables.
    ///
    /// This is false on Candle 0.11: the fused `layer_norm` kernel used by
    /// `BertModel` records `BackpropOp::none`, so no gradient reaches any
    /// encoder weight and the optimizer silently skips them. The M001A
    /// closure record owns this framework limitation; encoder unfreezing
    /// stays gated on a narrow framework corrective.
    pub top_layer_changed: bool,
    pub head_changed_in_top_stage: bool,
    pub lower_layers_unchanged: bool,
    pub pooling: PoolingSeparationReport,
    pub load_ms: u128,
    pub forward_ms: u128,
    pub head_step_ms: u128,
    pub top_layer_step_ms: u128,
    pub weights_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn hash(path: &Path) -> String {
        sha256_file(path).expect("hash")
    }

    fn write_tiny_fixture(dir: &Path) -> PathBuf {
        let config_path = dir.join("config.json");
        fs::write(
            &config_path,
            r#"{
                "vocab_size": 16,
                "hidden_size": 8,
                "num_hidden_layers": 2,
                "num_attention_heads": 2,
                "intermediate_size": 16,
                "hidden_act": "gelu",
                "hidden_dropout_prob": 0.0,
                "max_position_embeddings": 32,
                "type_vocab_size": 2,
                "initializer_range": 0.02,
                "layer_norm_eps": 0.000001,
                "pad_token_id": 0,
                "classifier_dropout": null,
                "model_type": "bert"
            }"#,
        )
        .expect("config");
        let config: Config = serde_json::from_slice(&fs::read(&config_path).expect("config read"))
            .expect("config parse");
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        let _model = BertModel::load(vb, &config).expect("tiny BERT");
        let weights_path = dir.join("weights.safetensors");
        varmap.save(&weights_path).expect("weights");
        let vocabulary_path = dir.join("vocab.txt");
        fs::write(
            &vocabulary_path,
            "[PAD]\n[UNK]\n[CLS]\n[SEP]\ninspect\nproject\nread\nfiles\nsend\nemail\n",
        )
        .expect("vocab");
        let license_path = dir.join("LICENSE.json");
        fs::write(
            &license_path,
            r#"{"license":"Apache-2.0","source":"fixture"}"#,
        )
        .expect("license");
        let manifest = AssetManifest {
            schema_version: ASSET_MANIFEST_SCHEMA_VERSION,
            architecture: ENCODER_ARCHITECTURE.into(),
            framework: "candle-0.11".into(),
            config_path: "config.json".into(),
            vocabulary_path: "vocab.txt".into(),
            weights_path: "weights.safetensors".into(),
            license_path: "LICENSE.json".into(),
            hashes: BTreeMap::from([
                ("config".into(), hash(&config_path)),
                ("vocabulary".into(), hash(&vocabulary_path)),
                ("weights".into(), hash(&weights_path)),
                ("license".into(), hash(&license_path)),
            ]),
        };
        let manifest_path = dir.join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .expect("manifest");
        manifest_path
    }

    #[test]
    fn tokenizer_is_deterministic_and_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vocab = dir.path().join("vocab.txt");
        fs::write(
            &vocab,
            "[UNK]\n[CLS]\n[SEP]\ninspect\nproject\nread\n##ing\n",
        )
        .expect("vocab");
        let tokenizer = WordPieceTokenizer::from_vocab(&vocab).expect("tokenizer");
        let first = tokenizer.encode_pair("inspect project", "reading", 5);
        let second = tokenizer.encode_pair("inspect project", "reading", 5);
        assert_eq!(first, second);
        assert_eq!(first.input_ids.len(), 5);
        assert_eq!(first.input_ids[0], 1);
        assert_eq!(*first.input_ids.last().expect("sep"), 2);
    }

    #[test]
    fn local_asset_hashes_are_required() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in [
            "config.json",
            "vocab.txt",
            "weights.safetensors",
            "LICENSE.json",
        ] {
            fs::write(dir.path().join(name), name).expect("asset");
        }
        let manifest = AssetManifest {
            schema_version: ASSET_MANIFEST_SCHEMA_VERSION,
            architecture: ENCODER_ARCHITECTURE.into(),
            framework: "candle-0.11".into(),
            config_path: "config.json".into(),
            vocabulary_path: "vocab.txt".into(),
            weights_path: "weights.safetensors".into(),
            license_path: "LICENSE.json".into(),
            hashes: BTreeMap::from([
                ("config".into(), hash(&dir.path().join("config.json"))),
                ("vocabulary".into(), hash(&dir.path().join("vocab.txt"))),
                (
                    "weights".into(),
                    hash(&dir.path().join("weights.safetensors")),
                ),
                ("license".into(), hash(&dir.path().join("LICENSE.json"))),
            ]),
        };
        let manifest_path = dir.path().join("manifest.json");
        let mut file = fs::File::create(&manifest_path).expect("manifest");
        file.write_all(&serde_json::to_vec(&manifest).expect("json"))
            .expect("write");
        assert!(ResolvedAssets::load(&manifest_path).is_ok());
        fs::write(dir.path().join("vocab.txt"), "changed").expect("tamper");
        assert!(ResolvedAssets::load(&manifest_path).is_err());
    }

    /// Candle 0.11 framework premise: the fused `layer_norm` kernel used by
    /// `BertModel` records `BackpropOp::none`, so encoder weights can never
    /// receive gradients through it, while the composite `layer_norm_slow`
    /// and the other BERT ops (softmax, index-select embeddings, gelu)
    /// propagate gradients normally. The M001A closure record owns the
    /// consequence (encoder unfreezing gated on a framework corrective);
    /// this test locks the premise so a future dependency change that
    /// restores encoder backward is noticed instead of silently assumed.
    #[test]
    fn candle_fused_layer_norm_has_no_backward() {
        use candle_nn::ops as nn_ops;
        let device = Device::Cpu;
        let input = Var::zeros((2usize, 4usize), DType::F32, &device).expect("var");
        let alpha = Var::zeros(4usize, DType::F32, &device).expect("var");
        let beta = Var::zeros(4usize, DType::F32, &device).expect("var");
        let fused =
            nn_ops::layer_norm(input.as_tensor(), alpha.as_tensor(), beta.as_tensor(), 1e-5)
                .expect("fused layer norm");
        let fused_grads = fused.mean_all().expect("mean").backward().expect("bwd");
        assert!(fused_grads.get(input.as_tensor()).is_none());
        assert!(fused_grads.get(alpha.as_tensor()).is_none());
        let slow_input = Var::zeros((2usize, 4usize), DType::F32, &device).expect("var");
        let slow_alpha = Var::zeros(4usize, DType::F32, &device).expect("var");
        let slow_beta = Var::zeros(4usize, DType::F32, &device).expect("var");
        let slow = nn_ops::layer_norm_slow(
            slow_input.as_tensor(),
            slow_alpha.as_tensor(),
            slow_beta.as_tensor(),
            1e-5,
        )
        .expect("slow layer norm");
        let slow_grads = slow.mean_all().expect("mean").backward().expect("bwd");
        assert!(slow_grads.get(slow_input.as_tensor()).is_some());
        assert!(slow_grads.get(slow_alpha.as_tensor()).is_some());
        assert!(slow_grads.get(slow_beta.as_tensor()).is_some());
        let sm_input = Var::zeros((2usize, 4usize), DType::F32, &device).expect("var");
        let sm = nn_ops::softmax(sm_input.as_tensor(), 1usize).expect("softmax");
        let sm_grads = sm.mean_all().expect("mean").backward().expect("bwd");
        assert!(sm_grads.get(sm_input.as_tensor()).is_some());
    }

    #[test]
    fn tiny_local_bert_proves_forward_and_staged_backward() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest = write_tiny_fixture(dir.path());
        let report = probe_local_assets(&manifest, &Device::Cpu).expect("probe");
        assert_eq!(report.architecture, ENCODER_ARCHITECTURE);
        assert_eq!(report.hidden_size, 8);
        assert_eq!(report.layers, 2);
        assert_eq!(report.deterministic_max_delta, 0.0);
        assert!(report.parameter_count > 0);
        assert!(report.head_only_loss.is_finite());
        assert!(report.top_layer_loss.is_finite());
        assert!(report.load_coverage.missing_variables.is_empty());
        assert!(!report.reference_config.matches_pinned_contract);
        assert_eq!(
            report.load_coverage.loaded_variables,
            report.load_coverage.expected_variables
        );
        assert!(report.embeddings_finite);
        assert!(report.head_only_encoder_unchanged);
        assert!(report.head_vars_changed);
        // Encoder weights cannot change on Candle 0.11: see
        // `candle_fused_layer_norm_has_no_backward` and the M001A closure
        // record. The probe facts below lock the honest outcome.
        assert!(!report.top_layer_changed);
        assert!(report.head_changed_in_top_stage);
        assert!(report.lower_layers_unchanged);
        assert!(report.pooling.all_finite_and_distinct);
        assert!(report.weights_bytes > 0);
    }

    fn minilm_like_config() -> Config {
        serde_json::from_value(serde_json::json!({
            "vocab_size": 30522,
            "hidden_size": 384,
            "num_hidden_layers": 6,
            "num_attention_heads": 12,
            "intermediate_size": 1536,
            "hidden_act": "gelu",
            "hidden_dropout_prob": 0.1,
            "max_position_embeddings": 512,
            "type_vocab_size": 2,
            "initializer_range": 0.02,
            "layer_norm_eps": 0.000000000001,
            "pad_token_id": 0,
            "model_type": "bert"
        }))
        .expect("reference config")
    }

    #[test]
    fn reference_config_validation_matches_pinned_minilm_contract() {
        assert!(validate_reference_checkpoint_config(&minilm_like_config()).is_ok());
        let mut wrong_size = minilm_like_config();
        wrong_size.hidden_size = 768;
        assert!(validate_reference_checkpoint_config(&wrong_size).is_err());
        let mut wrong_layers = minilm_like_config();
        wrong_layers.num_hidden_layers = 12;
        assert!(validate_reference_checkpoint_config(&wrong_layers).is_err());
        let mut wrong_heads = minilm_like_config();
        wrong_heads.num_attention_heads = 8;
        assert!(validate_reference_checkpoint_config(&wrong_heads).is_err());
        let mut wrong_vocab = minilm_like_config();
        wrong_vocab.vocab_size = 100;
        assert!(validate_reference_checkpoint_config(&wrong_vocab).is_err());
        let mut wrong_types = minilm_like_config();
        wrong_types.type_vocab_size = 1;
        assert!(validate_reference_checkpoint_config(&wrong_types).is_err());
        let mut wrong_positions = minilm_like_config();
        wrong_positions.max_position_embeddings = 256;
        assert!(validate_reference_checkpoint_config(&wrong_positions).is_err());
        let mut wrong_type = minilm_like_config();
        wrong_type.model_type = Some("roberta".into());
        assert!(validate_reference_checkpoint_config(&wrong_type).is_err());
        let mut missing_type = minilm_like_config();
        missing_type.model_type = None;
        assert!(validate_reference_checkpoint_config(&missing_type).is_err());
    }

    #[test]
    fn tiny_load_coverage_has_no_missing_variables() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest = write_tiny_fixture(dir.path());
        let encoder =
            CandleBertSequenceEncoder::load(&manifest, &Device::Cpu).expect("encoder load");
        let coverage =
            load_coverage(&encoder.assets.weights_path, &encoder.varmap).expect("coverage");
        assert!(coverage.missing_variables.is_empty());
        assert!(coverage.unexpected_source_tensors.is_empty());
        assert_eq!(coverage.loaded_variables, coverage.expected_variables);
        assert_eq!(
            coverage.loaded_parameter_count,
            coverage.source_parameter_count
        );
        assert!(coverage.loaded_parameter_count > 0);
    }

    #[test]
    fn pooling_strategies_are_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest = write_tiny_fixture(dir.path());
        let encoder =
            CandleBertSequenceEncoder::load(&manifest, &Device::Cpu).expect("encoder load");
        for strategy in [PoolingStrategy::Cls, PoolingStrategy::Mean] {
            let first = encoder
                .encode_with_pooling("inspect project", "read files", strategy)
                .expect("encode");
            let second = encoder
                .encode_with_pooling("inspect project", "read files", strategy)
                .expect("encode");
            assert_eq!(first, second);
            assert!(!first.is_empty());
            assert!(first.iter().all(|value| value.is_finite()));
        }
        let separation = encoder.pooling_separation().expect("separation");
        assert!(separation.all_finite_and_distinct);
    }
}
