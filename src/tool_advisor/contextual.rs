//! Optional pure-Rust contextual advisor runtime and trainer.
//!
//! This is deliberately self-contained instead of depending on a native ML
//! runtime. It uses deterministic hashed token embeddings plus a learned
//! context/candidate interaction score. The fixed-size embedding table gives
//! auditable small/medium capacity points while unseen tool names continue to
//! work through their textual descriptors.

use super::{
    sigmoid, split_for, tokenize, RankedCandidate, ToolAdvisor, ToolAdvisorCase, ToolAdvisorInput,
    ToolAdvisorPrediction, MAX_CONTEXT_BYTES, PREDICTION_SCHEMA_VERSION,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const ARTIFACT_MAGIC: &[u8] = b"CODEGG-TAE1";
pub const CONTEXTUAL_SCHEMA_VERSION: u16 = 1;
pub const VOCAB_BUCKETS: usize = 65_536;
pub const SMALL_EMBEDDING_DIM: usize = 80;
pub const MEDIUM_EMBEDDING_DIM: usize = 240;
pub const MAX_CONTEXT_TOKENS: usize = 96;
pub const MAX_CANDIDATE_TOKENS: usize = 128;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capacity {
    Small,
    Medium,
}

impl Capacity {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("small").to_ascii_lowercase().as_str() {
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            other => Err(anyhow!("unsupported contextual capacity {other}")),
        }
    }

    pub fn embedding_dim(self) -> usize {
        match self {
            Self::Small => SMALL_EMBEDDING_DIM,
            Self::Medium => MEDIUM_EMBEDDING_DIM,
        }
    }

    pub fn parameter_count(self) -> u64 {
        (VOCAB_BUCKETS * self.embedding_dim() + 1) as u64
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualTrainingConfig {
    pub capacity: Capacity,
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "default_max_candidates")]
    pub max_candidates: usize,
    #[serde(default)]
    pub model_version: Option<String>,
}

fn default_epochs() -> u32 {
    3
}

fn default_learning_rate() -> f32 {
    0.03
}

fn default_max_candidates() -> usize {
    32
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextualArtifactManifest {
    pub artifact_schema_version: u16,
    pub model_version: String,
    pub architecture: String,
    pub capacity: String,
    pub parameter_count: u64,
    pub embedding_dim: usize,
    pub vocab_buckets: usize,
    pub tokenizer_version: String,
    pub context_schema_version: u16,
    pub candidate_schema_version: u16,
    pub max_context_tokens: usize,
    pub max_candidate_tokens: usize,
    pub dataset_fingerprint: String,
    pub weights_sha256: String,
    pub provenance: String,
    pub license_notice: String,
}

#[derive(Debug, Clone)]
pub struct ContextualArtifact {
    pub manifest: ContextualArtifactManifest,
    pub bias: f32,
    weights: Vec<f32>,
}

impl ContextualArtifact {
    fn validate(&self) -> Result<()> {
        let manifest = &self.manifest;
        if manifest.artifact_schema_version != CONTEXTUAL_SCHEMA_VERSION
            || manifest.architecture != "contextual-embedding-v1"
            || manifest.vocab_buckets != VOCAB_BUCKETS
            || manifest.embedding_dim * manifest.vocab_buckets + 1
                != manifest.parameter_count as usize
            || self.weights.len() + 1 != manifest.parameter_count as usize
            || manifest.context_schema_version != super::CASE_SCHEMA_VERSION
            || manifest.candidate_schema_version != super::CASE_SCHEMA_VERSION
            || manifest.max_context_tokens == 0
            || manifest.max_context_tokens > MAX_CONTEXT_TOKENS
            || manifest.max_candidate_tokens == 0
            || manifest.max_candidate_tokens > MAX_CANDIDATE_TOKENS
        {
            return Err(anyhow!("contextual artifact manifest is incompatible"));
        }
        if !self.bias.is_finite() || self.weights.iter().any(|weight| !weight.is_finite()) {
            return Err(anyhow!("contextual artifact contains non-finite weights"));
        }
        let bytes = weights_bytes(&self.weights);
        let digest = hex::encode(Sha256::digest(bytes));
        if digest != manifest.weights_sha256 {
            return Err(anyhow!("contextual artifact weights hash mismatch"));
        }
        Ok(())
    }
}

pub struct ContextualAdvisor {
    artifact: Arc<ContextualArtifact>,
}

impl ContextualAdvisor {
    pub fn new(artifact: ContextualArtifact) -> Result<Self> {
        artifact.validate()?;
        Ok(Self {
            artifact: Arc::new(artifact),
        })
    }

    pub fn artifact(&self) -> &ContextualArtifact {
        &self.artifact
    }

    fn score_pair(&self, context: &str, candidate: &str) -> f32 {
        score_pair(
            &self.artifact.weights,
            self.artifact.manifest.embedding_dim,
            self.artifact.manifest.max_context_tokens,
            self.artifact.manifest.max_candidate_tokens,
            context,
            candidate,
        ) + self.artifact.bias
    }
}

impl ToolAdvisor for ContextualAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        if input.context.len() > MAX_CONTEXT_BYTES {
            return Err(anyhow!("contextual advisor context exceeds limit"));
        }
        if input.candidates.len() > super::MAX_CANDIDATES {
            return Err(anyhow!("contextual advisor candidate set exceeds limit"));
        }
        let mut ranked = input
            .candidates
            .iter()
            .map(|candidate| {
                let descriptor = format!(
                    "{} {} {} {}",
                    candidate.name, candidate.description, candidate.category, candidate.disclosure
                );
                (
                    candidate.name.clone(),
                    self.score_pair(&input.context, &descriptor) as f64,
                )
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        let abstain_probability = sigmoid(
            -ranked
                .first()
                .map(|(_, score)| *score as f32)
                .unwrap_or(0.0),
        );
        Ok(ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked: ranked
                .into_iter()
                .map(|(name, score)| RankedCandidate { name, score })
                .collect(),
            abstain_probability: Some(abstain_probability as f64),
            mode: "contextual".into(),
        })
    }
}

pub fn load(path: &Path) -> Result<ContextualArtifact> {
    let bytes =
        fs::read(path).with_context(|| format!("read contextual artifact {}", path.display()))?;
    if bytes.len() < ARTIFACT_MAGIC.len() + 4 || !bytes.starts_with(ARTIFACT_MAGIC) {
        return Err(anyhow!("not a contextual advisor artifact"));
    }
    let manifest_len_offset = ARTIFACT_MAGIC.len();
    let manifest_len = u32::from_le_bytes(
        bytes[manifest_len_offset..manifest_len_offset + 4]
            .try_into()
            .expect("fixed manifest length"),
    ) as usize;
    let manifest_start = manifest_len_offset + 4;
    let manifest_end = manifest_start
        .checked_add(manifest_len)
        .ok_or_else(|| anyhow!("contextual manifest length overflow"))?;
    if manifest_end + 4 > bytes.len() {
        return Err(anyhow!("truncated contextual artifact manifest"));
    }
    let manifest: ContextualArtifactManifest =
        serde_json::from_slice(&bytes[manifest_start..manifest_end])
            .context("parse contextual artifact manifest")?;
    let weight_bytes = &bytes[manifest_end + 4..];
    let expected = manifest
        .parameter_count
        .checked_sub(1)
        .ok_or_else(|| anyhow!("contextual artifact has no weights"))? as usize
        * 4;
    if weight_bytes.len() != expected {
        return Err(anyhow!("contextual artifact weight length mismatch"));
    }
    let bias = f32::from_le_bytes(
        bytes[manifest_end..manifest_end + 4]
            .try_into()
            .expect("fixed bias"),
    );
    let (weight_chunks, remainder) = weight_bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(anyhow!("contextual artifact has partial weight"));
    }
    let weights = weight_chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    let artifact = ContextualArtifact {
        manifest,
        bias,
        weights,
    };
    artifact.validate()?;
    Ok(artifact)
}

pub fn write_atomic(path: &Path, artifact: &ContextualArtifact) -> Result<()> {
    artifact.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("contextual artifact has no parent"))?;
    fs::create_dir_all(parent)?;
    let manifest = serde_json::to_vec(&artifact.manifest)?;
    let mut bytes = Vec::with_capacity(
        ARTIFACT_MAGIC.len() + 4 + manifest.len() + 4 + artifact.weights.len() * 4,
    );
    bytes.extend_from_slice(ARTIFACT_MAGIC);
    bytes.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&artifact.bias.to_le_bytes());
    bytes.extend_from_slice(&weights_bytes(&artifact.weights));
    let temp = path.with_extension("bin.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextualTrainingReport {
    pub artifact_path: String,
    pub capacity: String,
    pub parameter_count: u64,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub test_cases: usize,
    pub dataset_fingerprint: String,
    pub artifact_bytes: u64,
}

pub fn train(
    cases: &[ToolAdvisorCase],
    dataset_fingerprint: &str,
    config: &ContextualTrainingConfig,
    output: &Path,
) -> Result<ContextualTrainingReport> {
    if config.epochs == 0
        || config.epochs > 100
        || !config.learning_rate.is_finite()
        || config.learning_rate <= 0.0
        || config.max_candidates == 0
        || config.max_candidates > super::MAX_CANDIDATES
    {
        return Err(anyhow!("invalid contextual training configuration"));
    }
    let dim = config.capacity.embedding_dim();
    let mut weights = vec![0.0_f32; VOCAB_BUCKETS * dim];
    initialize(&mut weights, config.seed);
    let mut bias = 0.0_f32;
    let learning_cases = cases
        .iter()
        .filter(|case| split_for(case.split_group()) == "train")
        .collect::<Vec<_>>();
    let learning_cases = if learning_cases.is_empty() {
        cases.iter().collect::<Vec<_>>()
    } else {
        learning_cases
    };
    for _ in 0..config.epochs {
        for case in &learning_cases {
            for candidate in case.candidates.iter().take(config.max_candidates) {
                let descriptor = format!(
                    "{} {} {} {}",
                    candidate.name, candidate.description, candidate.category, candidate.disclosure
                );
                let score = score_pair(
                    &weights,
                    dim,
                    MAX_CONTEXT_TOKENS,
                    MAX_CANDIDATE_TOKENS,
                    &case.context,
                    &descriptor,
                ) + bias;
                let probability = sigmoid(score);
                let target = case.relevance.get(&candidate.name).copied().unwrap_or(0) as f32 / 3.0;
                let error = probability - target;
                update_pair(
                    &mut weights,
                    dim,
                    &case.context,
                    &descriptor,
                    -config.learning_rate * error,
                );
                bias -= config.learning_rate * error * 0.1;
            }
        }
    }
    let weights_sha256 = hex::encode(Sha256::digest(weights_bytes(&weights)));
    let manifest = ContextualArtifactManifest {
        artifact_schema_version: CONTEXTUAL_SCHEMA_VERSION,
        model_version: config
            .model_version
            .clone()
            .unwrap_or_else(|| format!("contextual-encoder-v1-{}", config.capacity.as_str())),
        architecture: "contextual-embedding-v1".into(),
        capacity: config.capacity.as_str().into(),
        parameter_count: config.capacity.parameter_count(),
        embedding_dim: dim,
        vocab_buckets: VOCAB_BUCKETS,
        tokenizer_version: "unicode-alnum-hash-v1".into(),
        context_schema_version: super::CASE_SCHEMA_VERSION,
        candidate_schema_version: super::CASE_SCHEMA_VERSION,
        max_context_tokens: MAX_CONTEXT_TOKENS,
        max_candidate_tokens: MAX_CANDIDATE_TOKENS,
        dataset_fingerprint: dataset_fingerprint.into(),
        weights_sha256,
        provenance: "local-rust-contextual-training".into(),
        license_notice: "MIT; trained from repository-local advisor fixtures".into(),
    };
    let artifact = ContextualArtifact {
        manifest,
        bias,
        weights,
    };
    write_atomic(output, &artifact)?;
    let metadata = fs::metadata(output)?;
    let train_cases = cases
        .iter()
        .filter(|case| split_for(case.split_group()) == "train")
        .count();
    let dev_cases = cases
        .iter()
        .filter(|case| split_for(case.split_group()) == "dev")
        .count();
    let test_cases = cases
        .iter()
        .filter(|case| split_for(case.split_group()) == "test")
        .count();
    Ok(ContextualTrainingReport {
        artifact_path: output.display().to_string(),
        capacity: config.capacity.as_str().into(),
        parameter_count: config.capacity.parameter_count(),
        train_cases,
        dev_cases,
        test_cases,
        dataset_fingerprint: dataset_fingerprint.into(),
        artifact_bytes: metadata.len(),
    })
}

fn score_pair(
    weights: &[f32],
    dim: usize,
    max_context_tokens: usize,
    max_candidate_tokens: usize,
    context: &str,
    candidate: &str,
) -> f32 {
    let context_tokens = token_indices(context, max_context_tokens);
    let candidate_tokens = token_indices(candidate, max_candidate_tokens);
    if context_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0.0;
    }
    let context_count = context_tokens.len() as f32;
    let candidate_count = candidate_tokens.len() as f32;
    let mut context_vector = vec![0.0; dim];
    let mut candidate_vector = vec![0.0; dim];
    for index in context_tokens {
        add_embedding(
            weights,
            dim,
            index,
            &mut context_vector,
            1.0 / context_count,
        );
    }
    for index in candidate_tokens {
        add_embedding(
            weights,
            dim,
            index,
            &mut candidate_vector,
            1.0 / candidate_count,
        );
    }
    context_vector
        .iter()
        .zip(candidate_vector)
        .map(|(left, right)| left * right)
        .sum::<f32>()
        / dim as f32
}

fn update_pair(weights: &mut [f32], dim: usize, context: &str, candidate: &str, scale: f32) {
    let context_tokens = token_indices(context, MAX_CONTEXT_TOKENS);
    let candidate_tokens = token_indices(candidate, MAX_CANDIDATE_TOKENS);
    if context_tokens.is_empty() || candidate_tokens.is_empty() {
        return;
    }
    let context_count = context_tokens.len() as f32;
    let candidate_count = candidate_tokens.len() as f32;
    let mut context_vector = vec![0.0; dim];
    let mut candidate_vector = vec![0.0; dim];
    for index in &context_tokens {
        add_embedding(
            weights,
            dim,
            *index,
            &mut context_vector,
            1.0 / context_count,
        );
    }
    for index in &candidate_tokens {
        add_embedding(
            weights,
            dim,
            *index,
            &mut candidate_vector,
            1.0 / candidate_count,
        );
    }
    for index in context_tokens {
        let start = index * dim;
        for offset in 0..dim {
            weights[start + offset] -= scale * candidate_vector[offset] / dim as f32;
        }
    }
    for index in candidate_tokens {
        let start = index * dim;
        for offset in 0..dim {
            weights[start + offset] -= scale * context_vector[offset] / dim as f32;
        }
    }
}

fn add_embedding(weights: &[f32], dim: usize, index: usize, output: &mut [f32], scale: f32) {
    let start = index * dim;
    for offset in 0..dim {
        output[offset] += weights[start + offset] * scale;
    }
}

fn token_indices(text: &str, limit: usize) -> Vec<usize> {
    tokenize(text)
        .into_iter()
        .take(limit)
        .map(|token| hash_token(&token))
        .collect()
}

fn hash_token(token: &str) -> usize {
    let mut hash = 2_166_136_261_u32;
    for byte in token.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    (hash as usize) % VOCAB_BUCKETS
}

fn initialize(weights: &mut [f32], seed: u64) {
    let mut state = seed | 1;
    for weight in weights {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *weight = ((state as i64 % 10_000) as f32 / 10_000.0) * 0.02;
    }
}

fn weights_bytes(weights: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(weights.len() * 4);
    for weight in weights {
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    bytes
}

pub fn capacity_report() -> Vec<(String, u64, usize)> {
    [Capacity::Small, Capacity::Medium]
        .into_iter()
        .map(|capacity| {
            (
                capacity.as_str().into(),
                capacity.parameter_count(),
                capacity.embedding_dim(),
            )
        })
        .collect()
}

pub fn artifact_path_for(output_dir: &Path, capacity: Capacity) -> PathBuf {
    output_dir.join(format!("contextual-{}.bin", capacity.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_points_are_in_declared_small_model_range() {
        let report = capacity_report();
        assert_eq!(report[0].1, 5_242_881);
        assert_eq!(report[1].1, 15_728_641);
        assert!(report
            .iter()
            .all(|(_, parameters, _)| (5_000_000..=25_000_000).contains(parameters)));
    }

    #[test]
    fn context_changes_pair_score() {
        let mut weights = vec![0.0; VOCAB_BUCKETS * SMALL_EMBEDDING_DIM];
        let context_index = hash_token("definition");
        let candidate_index = hash_token("lsp");
        weights[context_index * SMALL_EMBEDDING_DIM] = 1.0;
        weights[candidate_index * SMALL_EMBEDDING_DIM] = 1.0;
        let score = score_pair(
            &weights,
            SMALL_EMBEDDING_DIM,
            MAX_CONTEXT_TOKENS,
            MAX_CANDIDATE_TOKENS,
            "find definition",
            "lsp definition",
        );
        assert!(score > 0.0);
    }
}
