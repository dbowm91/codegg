//! M011: train-partition inferability audit for retrieval-signal M003.
//!
//! This module audits frozen optimizer labels only. It does not load the
//! encoder, inspect dev/test labels as optimizer examples, or train weights.

use super::retrieval_signal::{
    classify_inferability, extract_schema_fields, live_builtin_schema, query_explicit_support,
    signal_tokens, split_identifier_tokens, InferabilityClass, M006_PROTOCOL,
};
use super::{dataset_fingerprint, load_cases, partition_cases, ToolAdvisorCase};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const M011_PROTOCOL: &str = "retrieval-signal-m003-train-audit-v1";
pub const M011_RECEIPT_PATH: &str = "assets/tool-advisor/retrieval-signal-m003-train-audit.json";
pub const M011_OUTPUT_DIR: &str = "target/tool-advisor/retrieval-signal-m003-train-audit";
const EXPECTED_DATASET_SHA256: &str =
    "06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582";
const EXPECTED_DEV_PARTITION_SHA256: &str =
    "b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9";
const EXPECTED_M006_DECISION_SHA256: &str =
    "24e3783f1cc1c3d8934bac1592607702966112554db8ca1215087f90cfd09756";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrainLabelAudit {
    pub case_id: String,
    pub tool: String,
    pub grade: u8,
    pub class: String,
    pub rationale: String,
    pub supporting_query_text: String,
    pub optimizer_included: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct M011TrainAuditReceipt {
    pub schema_version: u16,
    pub protocol: String,
    pub m006_decision_sha256: String,
    pub dataset_sha256: String,
    pub dev_partition_sha256: String,
    pub train_partition_sha256: String,
    pub optimizer_input_sha256: String,
    pub train_cases: usize,
    pub train_cases_with_labels: usize,
    pub optimizer_cases: usize,
    pub labels_by_class: BTreeMap<String, usize>,
    pub labels_by_grade: BTreeMap<u8, usize>,
    pub labels: Vec<TrainLabelAudit>,
}

fn schema_cue_tokens(candidate_name: &str) -> Vec<String> {
    let fields = live_builtin_schema(candidate_name)
        .map(|schema| extract_schema_fields(&schema))
        .unwrap_or_default();
    fields
        .iter()
        .flat_map(|field| {
            split_identifier_tokens(&field.name)
                .into_iter()
                .chain(signal_tokens(&field.description))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn classify_label(
    case: &ToolAdvisorCase,
    tool: &str,
    grade: u8,
) -> Result<(InferabilityClass, String, String)> {
    let candidate = case
        .candidates
        .iter()
        .find(|candidate| candidate.name == tool)
        .ok_or_else(|| anyhow!("train relevance label references missing candidate {tool}"))?;
    let max_grade = case.relevance.values().copied().max().unwrap_or(grade);
    let (mut class, mut rationale, support) = classify_inferability(
        &case.context,
        &candidate.name,
        &candidate.description,
        &schema_cue_tokens(&candidate.name),
        grade,
        grade >= max_grade,
    );

    // Match M001's sibling-aware adjudication: a co-relevant tool that
    // explicitly explains the current query makes an otherwise unsupported
    // same-highest-grade label a supporting workflow step.
    if class == InferabilityClass::OtherEvidenceDefect {
        for (name, sibling_grade) in &case.relevance {
            if name == tool || *sibling_grade < grade {
                continue;
            }
            if let Some(sibling) = case.candidates.iter().find(|item| &item.name == name) {
                if let Some(sibling_support) =
                    query_explicit_support(&case.context, &sibling.name, &sibling.description)
                {
                    class = InferabilityClass::ImplicitSecondary;
                    rationale = format!(
                        "grade {grade} shares the case-highest grade with co-relevant '{name}' (query-explicit via '{sibling_support}'); no descriptor, schema, or paraphrase cue for '{tool}' is expressed, so the label represents a later/supporting workflow step not inferable from allowed query state"
                    );
                    break;
                }
            }
        }
    }
    Ok((class, rationale, support))
}

pub fn audited_optimizer_cases(
    cases: &[ToolAdvisorCase],
    train_indexes: &[usize],
) -> Result<(Vec<ToolAdvisorCase>, M011TrainAuditReceipt)> {
    let corpus_sha = dataset_fingerprint(cases)?;
    if corpus_sha != EXPECTED_DATASET_SHA256 {
        return Err(anyhow!(
            "M011 frozen corpus fingerprint changed: {corpus_sha}"
        ));
    }
    let train_cases: Vec<ToolAdvisorCase> = train_indexes
        .iter()
        .map(|index| {
            cases
                .get(*index)
                .cloned()
                .ok_or_else(|| anyhow!("train partition index {index} is out of range"))
        })
        .collect::<Result<_>>()?;
    // The supplied train index list must be exactly the partition's train
    // subset; callers cannot smuggle test or dev rows into optimizer inputs.
    let expected_partition = partition_cases(cases);
    if train_indexes != expected_partition.train_cases.as_slice() {
        return Err(anyhow!(
            "M011 train indexes differ from the frozen partition"
        ));
    }
    let expected_dev: Vec<ToolAdvisorCase> = expected_partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_sha = dataset_fingerprint(&expected_dev)?;
    if dev_sha != EXPECTED_DEV_PARTITION_SHA256 {
        return Err(anyhow!(
            "M011 frozen dev partition fingerprint changed: {dev_sha}"
        ));
    }
    let train_sha = dataset_fingerprint(&train_cases)?;
    let mut filtered = Vec::new();
    let mut labels = Vec::new();
    let mut labels_by_class = BTreeMap::new();
    let mut labels_by_grade = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut train_cases_with_labels = 0;
    for mut case in train_cases.iter().cloned() {
        if !case.relevance.is_empty() {
            train_cases_with_labels += 1;
        }
        let grades: Vec<(String, u8)> = case
            .relevance
            .iter()
            .map(|(tool, grade)| (tool.clone(), *grade))
            .collect();
        for (tool, grade) in grades {
            if !seen.insert((case.case_id.clone(), tool.clone())) {
                return Err(anyhow!(
                    "duplicate M011 optimizer pair {} / {tool}",
                    case.case_id
                ));
            }
            let (class, rationale, supporting_query_text) = classify_label(&case, &tool, grade)
                .with_context(|| format!("classify M011 train label {} / {tool}", case.case_id))?;
            if class == InferabilityClass::OtherEvidenceDefect {
                return Err(anyhow!(
                    "M011 other-evidence-defect for train label {} / {tool}: {rationale}",
                    case.case_id
                ));
            }
            if supporting_query_text.trim().is_empty() {
                return Err(anyhow!(
                    "M011 label {} / {tool} has no allowed query support",
                    case.case_id
                ));
            }
            let optimizer_included = class != InferabilityClass::ImplicitSecondary;
            *labels_by_class
                .entry(class.as_str().to_string())
                .or_default() += 1;
            *labels_by_grade.entry(grade).or_default() += 1;
            labels.push(TrainLabelAudit {
                case_id: case.case_id.clone(),
                tool: tool.clone(),
                grade,
                class: class.as_str().to_string(),
                rationale,
                supporting_query_text,
                optimizer_included,
            });
            if !optimizer_included {
                case.relevance.remove(&tool);
                case.preferred_order.retain(|name| name != &tool);
            }
        }
        // M003 preregistration omits no-tool and empty-positive cases from
        // projection optimization; they do not become implicit negatives.
        if !case.relevance.is_empty() {
            case.validate()
                .with_context(|| format!("validate M011 filtered train case {}", case.case_id))?;
            filtered.push(case);
        }
    }
    labels.sort_by(|left, right| (&left.case_id, &left.tool).cmp(&(&right.case_id, &right.tool)));
    let optimizer_input_sha = dataset_fingerprint(&filtered)?;
    let receipt = M011TrainAuditReceipt {
        schema_version: 1,
        protocol: M011_PROTOCOL.into(),
        m006_decision_sha256: EXPECTED_M006_DECISION_SHA256.into(),
        dataset_sha256: corpus_sha,
        dev_partition_sha256: dev_sha,
        train_partition_sha256: train_sha,
        optimizer_input_sha256: optimizer_input_sha,
        train_cases: train_cases.len(),
        train_cases_with_labels,
        optimizer_cases: filtered.len(),
        labels_by_class,
        labels_by_grade,
        labels,
    };
    Ok((filtered, receipt))
}

pub fn run_repository_audit(root: &Path) -> Result<M011TrainAuditReceipt> {
    let corpus_path = root.join("assets/tool-advisor/corpus.jsonl");
    let cases = load_cases(Some(&corpus_path))?;
    let partition = partition_cases(&cases);
    let (optimizer_cases, receipt) = audited_optimizer_cases(&cases, &partition.train_cases)?;
    let decision_path = root.join("assets/tool-advisor/retrieval-signal-m006-decision.json");
    let decision_bytes = fs::read(&decision_path)
        .with_context(|| format!("read frozen M006 decision {}", decision_path.display()))?;
    if hex::encode(sha2::Sha256::digest(&decision_bytes)) != EXPECTED_M006_DECISION_SHA256 {
        return Err(anyhow!("M011 frozen M006 decision receipt changed"));
    }
    let output_dir = root.join(M011_OUTPUT_DIR);
    fs::create_dir_all(&output_dir)?;
    fs::write(
        output_dir.join("optimizer-cases.jsonl"),
        optimizer_cases
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n")
            + "\n",
    )?;
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)?;
    fs::write(root.join(M011_RECEIPT_PATH), &receipt_bytes)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    #[test]
    fn m003_train_audit_fails_closed_on_unclassified_primary_label() {
        let cases = load_cases(None).expect("frozen corpus");
        let partition = partition_cases(&cases);
        let error = audited_optimizer_cases(&cases, &partition.train_cases)
            .expect_err("M011 must stop rather than admit an unsupported primary label");
        let message = format!("{error:#}");
        assert!(message.contains("filesystem-semantic-014-variant-1 / read"));
        assert!(message.contains("other-evidence-defect"));
    }

    #[test]
    fn optimizer_view_rejects_non_train_indexes() {
        let cases = load_cases(None).expect("frozen corpus");
        let partition = partition_cases(&cases);
        let mut indexes = partition.train_cases.clone();
        indexes.push(partition.dev_cases[0]);
        assert!(audited_optimizer_cases(&cases, &indexes).is_err());
    }

    #[test]
    fn m006_decision_fingerprint_is_frozen() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bytes = fs::read(root.join("assets/tool-advisor/retrieval-signal-m006-decision.json"))
            .expect("M006 decision");
        let actual = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, EXPECTED_M006_DECISION_SHA256);
        let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("M006 JSON");
        assert_eq!(receipt["protocol"], M006_PROTOCOL);
        assert_eq!(receipt["decision"]["target"], "current-step-only");
    }
}
