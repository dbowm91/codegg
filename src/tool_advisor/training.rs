//! Opt-in local trainer for the M002 `hashed-linear-v1` artifact.

use super::*;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const TRAINING_CONFIG_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingConfig {
    pub schema_version: u16,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub capacity: Option<crate::tool_advisor::contextual::Capacity>,
    /// Variable embedding-table size for contextual training. `None`
    /// preserves the historical 65,536 buckets.
    #[serde(default)]
    pub vocab_buckets: Option<usize>,
    /// Test-only escape hatch for fitting without C001 partitions.
    /// Output is permanently marked unqualified.
    #[serde(default)]
    pub allow_unpartitioned_fallback: bool,
    /// Tool families excluded from optimizer AND calibration input for true
    /// holdout retraining. Splits stay frozen; excluded cases are dropped
    /// from train/dev index lists and recorded in the report.
    #[serde(default)]
    pub exclude_tool_families: Vec<String>,
    #[serde(default)]
    pub dataset: Option<String>,
    pub output_artifact: String,
    #[serde(default)]
    pub run_dir: Option<String>,
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "default_max_context_bytes")]
    pub max_context_bytes: usize,
    #[serde(default = "default_max_candidates")]
    pub max_candidates: usize,
    #[serde(default = "default_temperature")]
    pub calibration_temperature: f32,
    #[serde(default = "default_true")]
    pub resume: bool,
}

fn default_epochs() -> u32 {
    3
}
fn default_learning_rate() -> f32 {
    0.05
}
fn default_max_context_bytes() -> usize {
    MAX_CONTEXT_BYTES
}
fn default_max_candidates() -> usize {
    16
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Checkpoint {
    schema_version: u16,
    epoch: u32,
    config_fingerprint: String,
    dataset_fingerprint: String,
    weights: BTreeMap<String, f32>,
    bias: f32,
    abstain_bias: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingReport {
    pub config_fingerprint: String,
    pub dataset_fingerprint: String,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub test_cases: usize,
    pub epochs_completed: u32,
    pub calibration_temperature: f32,
    pub artifact_path: String,
    pub artifact_bytes: u64,
    pub parameter_count: u64,
    pub train_metrics: MetricSummary,
    /// Dev metrics for tuning. Final-test metrics are never computed during
    /// training; they are `None` here and only populated by the explicit
    /// `evaluate_artifact` qualification path (C004).
    #[serde(default)]
    pub dev_metrics: Option<MetricSummary>,
    #[serde(default)]
    pub test_metrics: Option<MetricSummary>,
    /// Which split this report covers: `train+dev` for training output, or
    /// the requested `train`/`dev`/`test`/`all` for evaluation output.
    #[serde(default = "default_evaluation_partition")]
    pub evaluation_partition: String,
    /// True by design only for the `all` diagnostic evaluation, which must never back
    /// qualification evidence.
    #[serde(default)]
    pub evaluation_diagnostic_all: bool,
    /// Families excluded from this run's optimizer/calibration input.
    #[serde(default)]
    pub excluded_tool_families: Vec<String>,
}

fn default_evaluation_partition() -> String {
    "train+dev".into()
}

pub fn load_config(path: &Path) -> Result<TrainingConfig> {
    let bytes =
        fs::read(path).with_context(|| format!("read training config {}", path.display()))?;
    let config: TrainingConfig =
        if path.extension().and_then(|extension| extension.to_str()) == Some("toml") {
            toml::from_str(std::str::from_utf8(&bytes).context("training config is not UTF-8")?)
                .context("parse TOML training config")?
        } else {
            serde_json::from_slice(&bytes).context("parse JSON training config")?
        };
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &TrainingConfig) -> Result<()> {
    if config.schema_version != TRAINING_CONFIG_VERSION {
        return Err(anyhow!(
            "unsupported training config schema version {}",
            config.schema_version
        ));
    }
    if config.output_artifact.trim().is_empty() || config.epochs == 0 || config.epochs > 10_000 {
        return Err(anyhow!(
            "training config has invalid output or epoch bounds"
        ));
    }
    if !config.learning_rate.is_finite()
        || config.learning_rate <= 0.0
        || config.learning_rate > 10.0
    {
        return Err(anyhow!(
            "training learning_rate must be finite and in (0, 10]"
        ));
    }
    if config.max_context_bytes == 0
        || config.max_context_bytes > MAX_CONTEXT_BYTES
        || config.max_candidates == 0
        || config.max_candidates > MAX_CANDIDATES
    {
        return Err(anyhow!("training input limits exceed advisor bounds"));
    }
    if !config.calibration_temperature.is_finite() || config.calibration_temperature <= 0.0 {
        return Err(anyhow!(
            "calibration temperature must be positive and finite"
        ));
    }
    Ok(())
}

pub fn train(config: &TrainingConfig) -> Result<TrainingReport> {
    validate_config(config)?;
    let cases = load_cases(config.dataset.as_deref().map(Path::new))?;
    let dataset_fingerprint = dataset_fingerprint(&cases)?;
    let config_fingerprint = fingerprint_config(config)?;
    if config.architecture.as_deref() == Some("contextual-embedding-v1") {
        return train_contextual(config, &cases, &dataset_fingerprint, &config_fingerprint);
    }
    let output = PathBuf::from(&config.output_artifact);
    let run_dir = config
        .run_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| output.with_extension("run"));
    fs::create_dir_all(&run_dir)
        .with_context(|| format!("create training run directory {}", run_dir.display()))?;
    let lock_path = run_dir.join(".training.lock");
    let _lock = acquire_lock(&lock_path)?;
    let checkpoint_path = run_dir.join("checkpoint.json");
    let mut checkpoint = if config.resume && checkpoint_path.exists() {
        let checkpoint: Checkpoint = serde_json::from_slice(
            &fs::read(&checkpoint_path).context("read training checkpoint")?,
        )
        .context("parse training checkpoint")?;
        if checkpoint.config_fingerprint != config_fingerprint
            || checkpoint.dataset_fingerprint != dataset_fingerprint
        {
            return Err(anyhow!(
                "training checkpoint belongs to a different config or dataset"
            ));
        }
        checkpoint
    } else {
        Checkpoint {
            schema_version: TRAINING_CONFIG_VERSION,
            epoch: 0,
            config_fingerprint: config_fingerprint.clone(),
            dataset_fingerprint: dataset_fingerprint.clone(),
            weights: BTreeMap::new(),
            bias: 0.0,
            abstain_bias: 0.0,
        }
    };
    let layout = partition_cases(&cases);
    let mut excluded_families = config.exclude_tool_families.clone();
    excluded_families.sort();
    excluded_families.dedup();
    let excluded: BTreeSet<&str> = excluded_families.iter().map(String::as_str).collect();
    let train_cases: Vec<_> = layout
        .train_cases
        .iter()
        .map(|&index| cases[index].clone())
        .filter(|case| !excluded.contains(case.tool_family.as_str()))
        .collect();
    let dev_cases: Vec<_> = layout
        .dev_cases
        .iter()
        .map(|&index| cases[index].clone())
        .filter(|case| !excluded.contains(case.tool_family.as_str()))
        .collect();
    let test_cases: Vec<_> = layout
        .test_cases
        .iter()
        .map(|&index| cases[index].clone())
        .collect();
    // Qualification discipline: the optimizer sees train only, calibration
    // sees dev only, and the final test split is never scored during tuning.
    // The old all-case fallbacks are removed; degenerate corpora fail loudly.
    if train_cases.is_empty() {
        return Err(anyhow!(
            "training requires a non-empty C001 train partition; refusing all-case fallback"
        ));
    }
    if dev_cases.is_empty() {
        return Err(anyhow!(
            "training requires a non-empty C001 dev partition for calibration"
        ));
    }
    let learning_cases = train_cases.clone();
    while checkpoint.epoch < config.epochs {
        for case in &learning_cases {
            for candidate in &case.candidates {
                let features = tokenize(&format!("{} {}", candidate.name, candidate.description));
                if features.is_empty() {
                    continue;
                }
                let score = checkpoint.bias
                    + features
                        .iter()
                        .filter_map(|feature| checkpoint.weights.get(feature))
                        .copied()
                        .sum::<f32>();
                let probability = sigmoid(score);
                let target = case.relevance.get(&candidate.name).copied().unwrap_or(0) as f32 / 3.0;
                let error = probability - target;
                checkpoint.bias -=
                    config.learning_rate * error / learning_cases.len().max(1) as f32;
                for feature in features {
                    *checkpoint.weights.entry(feature).or_insert(0.0) -=
                        config.learning_rate * error / features_len(candidate) as f32;
                }
            }
            let probability = sigmoid(checkpoint.abstain_bias);
            let target = if case.none { 1.0 } else { 0.0 };
            checkpoint.abstain_bias -= config.learning_rate * (probability - target);
        }
        checkpoint.epoch += 1;
        write_checkpoint_atomic(&checkpoint_path, &checkpoint)?;
    }
    calibrate_abstention(&mut checkpoint, &dev_cases);
    write_checkpoint_atomic(&checkpoint_path, &checkpoint)?;
    let artifact = artifact_from_checkpoint(
        &checkpoint,
        config,
        &dataset_fingerprint,
        &excluded_families,
    )?;
    write_artifact_atomic(&output, &artifact)?;
    let runtime = LinearAdvisor::new(artifact.clone())?;
    let train_metrics = evaluate_cases(&runtime, &learning_cases, config)?;
    let dev_metrics = evaluate_cases(&runtime, &dev_cases, config)?;
    let artifact_bytes = fs::metadata(&output)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(TrainingReport {
        config_fingerprint,
        dataset_fingerprint,
        train_cases: learning_cases.len(),
        dev_cases: dev_cases.len(),
        test_cases: test_cases.len(),
        epochs_completed: checkpoint.epoch,
        calibration_temperature: config.calibration_temperature,
        artifact_path: output.display().to_string(),
        artifact_bytes,
        parameter_count: artifact.manifest.parameter_count,
        train_metrics,
        dev_metrics: Some(dev_metrics),
        test_metrics: None,
        evaluation_partition: default_evaluation_partition(),
        evaluation_diagnostic_all: false,
        excluded_tool_families: excluded_families,
    })
}

fn train_contextual(
    config: &TrainingConfig,
    cases: &[ToolAdvisorCase],
    dataset_fingerprint: &str,
    config_fingerprint: &str,
) -> Result<TrainingReport> {
    let contextual_config = crate::tool_advisor::contextual::ContextualTrainingConfig {
        capacity: config
            .capacity
            .unwrap_or(crate::tool_advisor::contextual::Capacity::Small),
        epochs: config.epochs,
        learning_rate: config.learning_rate,
        seed: config.seed,
        max_candidates: config.max_candidates,
        model_version: None,
        vocab_buckets: config.vocab_buckets,
        exclude_tool_families: config.exclude_tool_families.clone(),
        allow_unpartitioned_fallback: config.allow_unpartitioned_fallback,
    };
    let output = PathBuf::from(&config.output_artifact);
    let report = crate::tool_advisor::contextual::train(
        cases,
        dataset_fingerprint,
        &contextual_config,
        &output,
    )?;
    Ok(TrainingReport {
        config_fingerprint: config_fingerprint.to_string(),
        dataset_fingerprint: dataset_fingerprint.to_string(),
        train_cases: report.train_cases,
        dev_cases: report.dev_cases,
        test_cases: report.test_cases,
        epochs_completed: config.epochs,
        calibration_temperature: report.calibration.temperature,
        artifact_path: report.artifact_path,
        artifact_bytes: report.artifact_bytes,
        parameter_count: report.parameter_count,
        train_metrics: report.train_metrics,
        dev_metrics: Some(report.dev_metrics),
        test_metrics: None,
        evaluation_partition: default_evaluation_partition(),
        evaluation_diagnostic_all: false,
        excluded_tool_families: report.excluded_tool_families.clone(),
    })
}

/// Explicit-partition evaluation. Qualification requires naming one frozen
/// partition (default `test` at the CLI); `all` exists only for diagnostics
/// and its report is flagged so C004 can never mistake it for evidence.
pub fn evaluate_artifact(
    path: &Path,
    dataset: Option<&Path>,
    partition: &str,
) -> Result<TrainingReport> {
    let valid = matches!(partition, "train" | "dev" | "test" | "all");
    if !valid {
        return Err(anyhow!(
            "evaluation partition must be one of train, dev, test, all"
        ));
    }
    #[cfg(feature = "tool-advisor")]
    if let Ok(artifact) = crate::tool_advisor::contextual::load(path) {
        let cases = load_cases(dataset)?;
        let advisor = crate::tool_advisor::contextual::ContextualAdvisor::new(artifact)?;
        let selected: Vec<ToolAdvisorCase> = if partition == "all" {
            cases.clone()
        } else {
            let layout = partition_cases(&cases);
            let indices = match partition {
                "train" => &layout.train_cases,
                "dev" => &layout.dev_cases,
                _ => &layout.test_cases,
            };
            indices.iter().map(|&index| cases[index].clone()).collect()
        };
        if selected.is_empty() {
            return Err(anyhow!(
                "evaluation partition {partition} is empty; refusing to score nothing"
            ));
        }
        let predictions = selected
            .iter()
            .map(|case| {
                let mut prediction = advisor.score(&ToolAdvisorInput {
                    case_id: case.case_id.clone(),
                    context: case.context.clone(),
                    candidates: case.candidates.clone(),
                    surface_fingerprint: dataset_fingerprint(std::slice::from_ref(case))?,
                })?;
                prediction.mode = format!("contextual-eval-{partition}");
                Ok(prediction)
            })
            .collect::<Result<Vec<_>>>()?;
        let metrics = evaluate(&selected, &predictions)?;
        return Ok(TrainingReport {
            config_fingerprint: "contextual-evaluation-only".into(),
            dataset_fingerprint: dataset_fingerprint(&cases)?,
            train_cases: 0,
            dev_cases: 0,
            test_cases: selected.len(),
            epochs_completed: 0,
            calibration_temperature: advisor
                .artifact()
                .manifest
                .calibration
                .as_ref()
                .map(|calibration| calibration.temperature)
                .unwrap_or(1.0),
            artifact_path: path.display().to_string(),
            artifact_bytes: fs::metadata(path)?.len(),
            parameter_count: advisor.artifact().manifest.parameter_count,
            train_metrics: metrics.clone(),
            dev_metrics: None,
            test_metrics: Some(metrics),
            evaluation_partition: partition.to_string(),
            evaluation_diagnostic_all: partition == "all",
            excluded_tool_families: Vec::new(),
        });
    }
    let artifact = load_artifact(path)?;
    let runtime = LinearAdvisor::new(artifact.clone())?;
    let cases = load_cases(dataset)?;
    let selected: Vec<ToolAdvisorCase> = if partition == "all" {
        cases.clone()
    } else {
        let layout = partition_cases(&cases);
        let indices = match partition {
            "train" => &layout.train_cases,
            "dev" => &layout.dev_cases,
            _ => &layout.test_cases,
        };
        indices.iter().map(|&index| cases[index].clone()).collect()
    };
    if selected.is_empty() {
        return Err(anyhow!(
            "evaluation partition {partition} is empty; refusing to score nothing"
        ));
    }
    let metrics = evaluate_cases(
        &runtime,
        &selected,
        &TrainingConfig {
            schema_version: TRAINING_CONFIG_VERSION,
            architecture: None,
            capacity: None,
            vocab_buckets: None,
            exclude_tool_families: Vec::new(),
            allow_unpartitioned_fallback: false,
            dataset: None,
            output_artifact: path.display().to_string(),
            run_dir: None,
            epochs: 1,
            learning_rate: 0.1,
            seed: 0,
            max_context_bytes: artifact.manifest.max_context_bytes,
            max_candidates: artifact.manifest.max_candidates,
            calibration_temperature: artifact.temperature,
            resume: false,
        },
    )?;
    Ok(TrainingReport {
        config_fingerprint: "evaluation-only".into(),
        dataset_fingerprint: dataset_fingerprint(&cases)?,
        train_cases: 0,
        dev_cases: 0,
        test_cases: selected.len(),
        epochs_completed: 0,
        calibration_temperature: artifact.temperature,
        artifact_path: path.display().to_string(),
        artifact_bytes: fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        parameter_count: artifact.manifest.parameter_count,
        train_metrics: metrics.clone(),
        dev_metrics: None,
        test_metrics: Some(metrics),
        evaluation_partition: partition.to_string(),
        evaluation_diagnostic_all: partition == "all",
        excluded_tool_families: Vec::new(),
    })
}

fn evaluate_cases(
    advisor: &LinearAdvisor,
    cases: &[ToolAdvisorCase],
    config: &TrainingConfig,
) -> Result<MetricSummary> {
    let predictions = cases
        .iter()
        .map(|case| {
            advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case
                    .context
                    .chars()
                    .take(config.max_context_bytes)
                    .collect(),
                candidates: case
                    .candidates
                    .iter()
                    .take(config.max_candidates)
                    .cloned()
                    .collect(),
                surface_fingerprint: "training".into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    evaluate(cases, &predictions)
}

fn artifact_from_checkpoint(
    checkpoint: &Checkpoint,
    config: &TrainingConfig,
    dataset_fingerprint: &str,
    excluded_tool_families: &[String],
) -> Result<ToolAdvisorArtifact> {
    let weights_sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&checkpoint.weights)?));
    Ok(ToolAdvisorArtifact {
        manifest: ToolAdvisorArtifactManifest {
            artifact_schema_version: ARTIFACT_SCHEMA_VERSION,
            model_version: format!("linear-{}", &dataset_fingerprint[..12]),
            architecture: "hashed-linear-v1".into(),
            parameter_count: checkpoint.weights.len() as u64 + 2,
            precision: "f32".into(),
            tokenizer_version: "unicode-alnum-v1".into(),
            tokenizer_hash: hex::encode(Sha256::digest(b"unicode-alnum-v1")),
            candidate_schema_version: CASE_SCHEMA_VERSION,
            context_schema_version: CASE_SCHEMA_VERSION,
            calibration_version: "temperature-v1".into(),
            max_context_bytes: config.max_context_bytes,
            max_candidates: config.max_candidates,
            weights_sha256,
            provenance_fingerprint: dataset_fingerprint.into(),
            license_notice: "CodeGG-generated local artifact".into(),
            excluded_tool_families: excluded_tool_families.to_vec(),
        },
        bias: checkpoint.bias,
        abstain_bias: checkpoint.abstain_bias,
        temperature: config.calibration_temperature,
        weights: checkpoint.weights.clone(),
    })
}

fn calibrate_abstention(checkpoint: &mut Checkpoint, cases: &[ToolAdvisorCase]) {
    let mut thresholds = vec![0.0_f32];
    for case in cases {
        thresholds.push(top_score(checkpoint, case));
    }
    thresholds.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mut best = (f32::MIN, checkpoint.abstain_bias);
    for threshold in thresholds {
        let mut true_positive = 0.0;
        let mut predicted_positive = 0.0;
        let mut actual_positive = 0.0;
        for case in cases {
            let predicted = top_score(checkpoint, case) <= threshold;
            if predicted {
                predicted_positive += 1.0;
            }
            if case.none {
                actual_positive += 1.0;
            }
            if predicted && case.none {
                true_positive += 1.0;
            }
        }
        let precision = if predicted_positive > 0.0 {
            true_positive / predicted_positive
        } else {
            0.0
        };
        let recall = if actual_positive > 0.0 {
            true_positive / actual_positive
        } else {
            0.0
        };
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };
        if f1 > best.0 {
            best = (f1, threshold);
        }
    }
    checkpoint.abstain_bias = best.1;
}

fn top_score(checkpoint: &Checkpoint, case: &ToolAdvisorCase) -> f32 {
    case.candidates
        .iter()
        .map(|candidate| {
            checkpoint.bias
                + tokenize(&format!("{} {}", candidate.name, candidate.description))
                    .iter()
                    .filter_map(|feature| checkpoint.weights.get(feature))
                    .copied()
                    .sum::<f32>()
        })
        .fold(f32::MIN, f32::max)
}

fn fingerprint_config(config: &TrainingConfig) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(config)?)))
}

fn features_len(candidate: &ToolAdvisorCandidate) -> usize {
    tokenize(&format!("{} {}", candidate.name, candidate.description))
        .len()
        .max(1)
}

fn write_checkpoint_atomic(path: &Path, checkpoint: &Checkpoint) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    let mut file =
        File::create(&temp).with_context(|| format!("create checkpoint {}", temp.display()))?;
    file.write_all(&serde_json::to_vec_pretty(checkpoint)?)
        .context("write checkpoint")?;
    file.sync_all().context("sync checkpoint")?;
    fs::rename(&temp, path).with_context(|| format!("install checkpoint {}", path.display()))?;
    Ok(())
}

impl Drop for TrainingLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct TrainingLock {
    path: PathBuf,
}

fn acquire_lock(path: &Path) -> Result<TrainingLock> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "training run is already active or lock is stale: {}",
                path.display()
            )
        })?;
    Ok(TrainingLock {
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_training_run_is_deterministic_and_emits_runtime_artifact() {
        let directory = tempfile::tempdir().expect("training temp directory");
        let artifact = directory.path().join("model.json");
        let config = TrainingConfig {
            schema_version: TRAINING_CONFIG_VERSION,
            architecture: None,
            capacity: None,
            vocab_buckets: None,
            exclude_tool_families: Vec::new(),
            allow_unpartitioned_fallback: false,
            dataset: None,
            output_artifact: artifact.display().to_string(),
            run_dir: Some(directory.path().join("run").display().to_string()),
            epochs: 2,
            learning_rate: 0.05,
            seed: 42,
            max_context_bytes: MAX_CONTEXT_BYTES,
            max_candidates: 16,
            calibration_temperature: 1.0,
            resume: false,
        };
        let first = train(&config).expect("first train");
        assert!(first.parameter_count >= 2);
        assert!(artifact.exists());
        let loaded = load_artifact(&artifact).expect("runtime artifact");
        assert_eq!(loaded.manifest.architecture, "hashed-linear-v1");
    }

    #[test]
    fn invalid_dataset_and_checkpoint_collisions_fail_safely() {
        let directory = tempfile::tempdir().expect("training temp directory");
        let config = TrainingConfig {
            schema_version: TRAINING_CONFIG_VERSION,
            architecture: None,
            capacity: None,
            vocab_buckets: None,
            exclude_tool_families: Vec::new(),
            allow_unpartitioned_fallback: false,
            dataset: Some(directory.path().join("missing.jsonl").display().to_string()),
            output_artifact: directory.path().join("model.json").display().to_string(),
            run_dir: None,
            epochs: 1,
            learning_rate: 0.05,
            seed: 1,
            max_context_bytes: MAX_CONTEXT_BYTES,
            max_candidates: 16,
            calibration_temperature: 1.0,
            resume: false,
        };
        assert!(train(&config).is_err());
    }
}
