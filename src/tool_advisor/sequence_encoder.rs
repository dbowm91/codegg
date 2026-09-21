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
use candle_transformers::models::bert::{BertModel, Config};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        let encoded = self
            .tokenizer
            .encode_pair(context, candidate, MAX_PAIR_TOKENS);
        let input_ids = Tensor::new(encoded.input_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let token_type_ids =
            Tensor::new(encoded.token_type_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let attention_mask =
            Tensor::new(encoded.attention_mask.as_slice(), &self.device)?.unsqueeze(0)?;
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;
        Ok(hidden
            .narrow(1, 0, 1)?
            .squeeze(1)?
            .squeeze(0)?
            .to_dtype(DType::F32)?
            .to_vec1()?)
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
pub fn probe_local_assets(manifest_path: &Path, device: &Device) -> Result<ProbeReport> {
    let encoder = CandleBertSequenceEncoder::load(manifest_path, device)?;
    let first = encoder.encode("inspect the project", "read repository files")?;
    let second = encoder.encode("inspect the project", "read repository files")?;
    let max_delta = first
        .iter()
        .zip(second.iter())
        .map(|(left, right)| (left - right).abs())
        .fold(0.0f32, f32::max);
    let head = ScalarRankingHead::new(encoder.config.hidden_size, device)?;
    let mut optimizer = encoder.optimizer_for_stage(&head, FineTuneStage::HeadOnly, 0.01)?;
    let head_loss = encoder.train_step(
        &head,
        &mut optimizer,
        ("inspect", "read files"),
        ("inspect", "send email"),
    )?;
    let mut top_optimizer =
        encoder.optimizer_for_stage(&head, FineTuneStage::TopLayers(1), 0.001)?;
    let top_layer_loss = encoder.train_step(
        &head,
        &mut top_optimizer,
        ("inspect", "read files"),
        ("inspect", "send email"),
    )?;
    Ok(ProbeReport {
        architecture: ENCODER_ARCHITECTURE.into(),
        hidden_size: encoder.config.hidden_size,
        layers: encoder.config.num_hidden_layers,
        vocab_size: encoder.config.vocab_size,
        deterministic_max_delta: max_delta,
        head_only_loss: head_loss,
        top_layer_loss,
        parameter_count: encoder
            .varmap
            .all_vars()
            .iter()
            .map(|var| var.elem_count() as u64)
            .sum(),
        weight_sha256: encoder
            .assets
            .manifest
            .hashes
            .get("weights")
            .cloned()
            .unwrap_or_default(),
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
    }
}
