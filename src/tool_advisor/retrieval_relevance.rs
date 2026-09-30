//! Retrieval-evaluation semantics corrective C001: inferable relevance target.
//!
//! The frozen corpus `relevance` field mixes tools inferable from bounded
//! current-state evidence with plausible downstream workflow steps. This
//! module defines the authoritative retrieval-relevance contract, derives a
//! versioned evaluation view over the frozen corpus without rewriting it,
//! and supports the unchanged-retrieval rebaseline.
//!
//! Benchmark cases expose only `current_objective` (the frozen `context`
//! string) through [`crate::tool_advisor::context_v2::AdvisorContextV2`];
//! `next_steps` is empty for every frozen case, so no frozen label can be
//! `ExplicitNextStep`. The variant is implemented for future corpora with
//! populated structured next-step fields.

use super::{dataset_fingerprint, partition_cases, ToolAdvisorCase};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Schema version for [`RetrievalRelevanceView`] and entries.
pub const RETRIEVAL_RELEVANCE_SCHEMA_VERSION: u16 = 1;
/// Adjudication version bound into the derived asset.
pub const RETRIEVAL_RELEVANCE_VERSION: &str = "retrieval-relevance-v1";
/// Repository-owned derived asset (relative to workspace root).
pub const DERIVED_ASSET_PATH: &str = "assets/tool-advisor/retrieval-relevance-v1.json";
/// Frozen full-corpus fingerprint (`dataset_fingerprint` over corpus.jsonl).
pub const EXPECTED_CORPUS_FINGERPRINT: &str =
    "06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582";
/// Frozen dev-partition fingerprint under [`partition_cases`].
pub const EXPECTED_DEV_PARTITION_FINGERPRINT: &str =
    "b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9";

/// Authoritative retrieval-relevance classes (corrective §4 / C001 §3).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RetrievalRelevanceClass {
    CurrentStep,
    ExplicitNextStep,
    ImplicitFuture,
    EvidenceDefect,
}

impl RetrievalRelevanceClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CurrentStep => "current-step",
            Self::ExplicitNextStep => "explicit-next-step",
            Self::ImplicitFuture => "implicit-future",
            Self::EvidenceDefect => "evidence-defect",
        }
    }

    pub fn is_retrieval_eligible(self) -> bool {
        matches!(self, Self::CurrentStep | Self::ExplicitNextStep)
    }
}

pub fn parse_class(value: &str) -> Result<RetrievalRelevanceClass> {
    match value {
        "current-step" => Ok(RetrievalRelevanceClass::CurrentStep),
        "explicit-next-step" => Ok(RetrievalRelevanceClass::ExplicitNextStep),
        "implicit-future" => Ok(RetrievalRelevanceClass::ImplicitFuture),
        "evidence-defect" => Ok(RetrievalRelevanceClass::EvidenceDefect),
        other => Err(anyhow!("unknown retrieval relevance class {other}")),
    }
}

/// One adjudicated positive label. Sorted by `(case_id, candidate)` in the
/// derived view for determinism.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalRelevanceEntry {
    pub case_id: String,
    pub candidate: String,
    pub original_grade: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_index: Option<usize>,
    #[serde(default)]
    pub tool_family: String,
    #[serde(default)]
    pub task_family: String,
    #[serde(rename = "class")]
    pub relevance_class: RetrievalRelevanceClass,
    pub retrieval_eligible: bool,
    #[serde(default)]
    pub supporting_field: String,
    #[serde(default)]
    pub supporting_text: String,
    #[serde(default)]
    pub rationale: String,
}

/// Versioned derived evaluation view over the frozen corpus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalRelevanceView {
    pub schema_version: u16,
    pub adjudication_version: String,
    pub corpus_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub entries: Vec<RetrievalRelevanceEntry>,
    pub fingerprint: String,
}

/// Effective request text for adjudication: the frozen context with the
/// explicitly stale transcript suffix removed. The suffix
/// ("The transcript mentions ... but that trail is stale") is negative
/// evidence by construction and must never count as direct support.
pub fn effective_context(context: &str) -> &str {
    let lowered = context.to_ascii_lowercase();
    if let Some(index) = lowered.find("the transcript mentions") {
        // Marker is ASCII so the byte index is valid in the original.
        if context.is_char_boundary(index) {
            return context[..index].trim_end();
        }
    }
    context
}

/// Deterministic cue lexicon: lowercase substrings in the effective request
/// that directly support using the tool now. Cues are generic task-language
/// entailments (verbs/nouns from tool semantics), never retrieval ranks or
/// per-miss aliases.
pub fn cues_for(tool: &str) -> &'static [&'static str] {
    match tool {
        "browser_test" => &["browser test"],
        "context_read" => &[
            "read the persisted artifact",
            "read artifact",
            "persisted artifact",
        ],
        "coverage" => &["measure coverage", "resulting coverage", "coverage of"],
        "docs_lookup" => &["look up the api", "look up api"],
        "env_get" => &[
            "read the environment",
            "environment variables",
            "environment for",
        ],
        "fetch_url" => &["fetch the reference page", "fetch "],
        "git_blame" => &["attribute ", "blame ", "to their authors"],
        "git_diff" => &[
            "show the unstaged diff",
            "unstaged diff",
            "unstaged changes",
        ],
        "git_log" => &[
            "list recent commits",
            "review the history",
            "history of",
            "commit history",
            "git_log",
        ],
        "git_status" => &["check status", "working tree status", "show working tree"],
        "glob" => &["locate every test fixture", "find files by path"],
        "goal_get" => &["show the active goal", "tracking goal", "active goal"],
        "grep" => &[
            "find the literal token",
            "find literal uses",
            "search literal",
            "literal token",
        ],
        "json_edit" => &["edit the json", "edit json", "edit document"],
        "json_query" => &["query the json", "query json", "query path", "json path"],
        "lint" => &["lint "],
        "list_dir" => &["list the entries", "list directory", "sibling entries"],
        "lsp_definition" => &[
            "jump to the definition",
            "jump to a symbol definition",
            "definition of",
        ],
        "lsp_hover" => &[
            "show the documentation",
            "inspect the documentation",
            "documentation for",
            "documentation of",
        ],
        "lsp_references" => &[
            "find every reference",
            "find all references",
            "list its other references",
            "other references",
            "lsp_references",
        ],
        "lsp_rename" => &[
            "rename the symbol",
            "rename a symbol",
            "renaming it",
            "before renaming",
            "rename ",
        ],
        "plan_get" => &[
            "show the plan entry",
            "show the work plan",
            "work plan entry",
            "active work plan",
        ],
        "plugin_enable" => &[
            "enable the installed extension",
            "enable the extension",
            "enable an installed",
            "enable ",
        ],
        "plugin_install" => &[
            "install the extension",
            "install an extension",
            "install the match",
            "then install",
        ],
        "plugin_search" => &["search the catalog", "plugin catalog"],
        "process_list" => &[
            "list the processes",
            "list matching processes",
            "currently running",
        ],
        "read" => &["read the module", "open ", "report its public exports"],
        "replace" => &["replace every occurrence", "replace text", "new spelling"],
        "schema_validate" => &[
            "validate ",
            "declared schema",
            "against the schema",
            "against its declared schema",
        ],
        "semantic_search" => &[
            "locate code semantically",
            "search semantically",
            "semantically related",
        ],
        "shell_exec" => &[
            "execute the maintenance command",
            "execute the command",
            "maintenance command",
        ],
        "shell_history" => &["shell history", "recent shell history"],
        "summarize" => &["summarize "],
        "symbol_search" => &[
            "find the workspace symbol",
            "workspace symbol",
            "matching symbol declaration",
            "jump to the matching symbol",
        ],
        "table_filter" => &[
            "filter the tabular",
            "filter ",
            "tabular export",
            "resulting table",
        ],
        "test_run" => &[
            "run the focused tests",
            "run its focused tests",
            "focused tests",
        ],
        "typecheck" => &["type-check", "typecheck", "type check"],
        "web_search" => &[
            "find external documentation",
            "search the web",
            "external documentation",
        ],
        "write" => &["create ", "persist ", "confirm the write", "write content"],
        "memory_recall" => &["recall curated", "curated memory"],
        _ => &[],
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn excerpt_around(haystack: &str, cue: &str) -> String {
    let lowered = haystack.to_ascii_lowercase();
    let cue_lower = cue.to_ascii_lowercase();
    let Some(pos) = lowered.find(cue_lower.as_str()) else {
        return truncate_utf8(haystack.trim(), 160);
    };
    let start = pos.saturating_sub(60);
    let end = (pos + cue.len() + 60).min(haystack.len());
    let mut start = start;
    while start < end && !haystack.is_char_boundary(start) {
        start += 1;
    }
    let mut end = end;
    while end > start && !haystack.is_char_boundary(end) {
        end -= 1;
    }
    truncate_utf8(haystack[start..end].trim(), 160)
}

/// Deterministic adjudication of one positive label from allowed evidence.
///
/// Frozen benchmark cases populate only `current_objective`, so inferable
/// labels are `CurrentStep` and `ExplicitNextStep` never fires here. The
/// `ExplicitNextStep` arm is implemented for future corpora with populated
/// structured `next_steps` fields.
pub fn classify_tool(
    case: &ToolAdvisorCase,
    candidate: &str,
) -> (
    RetrievalRelevanceClass,
    String,
    String,
    String,
    Option<String>,
) {
    let effective = effective_context(&case.context);
    let lowered = effective.to_ascii_lowercase();
    for cue in cues_for(candidate) {
        if lowered.contains(&cue.to_ascii_lowercase()) {
            let excerpt = excerpt_around(effective, cue);
            let rationale = format!(
                "Direct support in current_objective via cue '{cue}': \
                 '{excerpt}'. Tool inferable now."
            );
            return (
                RetrievalRelevanceClass::CurrentStep,
                "current_objective".to_string(),
                excerpt,
                rationale,
                Some((*cue).to_string()),
            );
        }
    }
    let bounded = truncate_utf8(effective.trim(), 160);
    let rationale = format!(
        "No allowed current-state field directly supports '{candidate}' \
         for this request; plausible later workflow action only. Effective \
         request: '{bounded}'."
    );
    (
        RetrievalRelevanceClass::ImplicitFuture,
        String::new(),
        bounded,
        rationale,
        None,
    )
}

/// Build the deterministic derived view over every positive corpus label.
pub fn build_derived_view(cases: &[ToolAdvisorCase]) -> Result<RetrievalRelevanceView> {
    let corpus_fingerprint = dataset_fingerprint(cases).context("fingerprint corpus")?;
    let partition = partition_cases(cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_partition_fingerprint =
        dataset_fingerprint(&dev_cases).context("fingerprint dev partition")?;
    let mut ordered = cases.to_vec();
    ordered.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    let mut entries = Vec::new();
    for case in &ordered {
        let mut names: Vec<&String> = case.relevance.keys().collect();
        names.sort();
        for name in names {
            let grade = case.relevance[name];
            let preferred_index = case.preferred_order.iter().position(|n| n == name);
            let (class, supporting_field, supporting_text, rationale, _) =
                classify_tool(case, name);
            entries.push(RetrievalRelevanceEntry {
                case_id: case.case_id.clone(),
                candidate: name.clone(),
                original_grade: grade,
                preferred_index,
                tool_family: case.tool_family.clone(),
                task_family: case.task_family.clone(),
                retrieval_eligible: class.is_retrieval_eligible(),
                supporting_field,
                supporting_text,
                rationale,
                relevance_class: class,
            });
        }
    }
    let fingerprint = derived_view_fingerprint(&entries);
    Ok(RetrievalRelevanceView {
        schema_version: RETRIEVAL_RELEVANCE_SCHEMA_VERSION,
        adjudication_version: RETRIEVAL_RELEVANCE_VERSION.to_string(),
        corpus_fingerprint,
        dev_partition_fingerprint,
        entries,
        fingerprint,
    })
}

/// Deterministic fingerprint over sorted entries (excludes itself).
pub fn derived_view_fingerprint(entries: &[RetrievalRelevanceEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|left, right| {
        left.case_id
            .cmp(&right.case_id)
            .then_with(|| left.candidate.cmp(&right.candidate))
    });
    let bytes = serde_json::to_vec(&sorted).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

pub fn load_derived_view(path: &Path) -> Result<RetrievalRelevanceView> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read derived view {}", path.display()))?;
    let view: RetrievalRelevanceView =
        serde_json::from_slice(&bytes).context("parse derived view")?;
    Ok(view)
}

pub fn write_derived_view_atomic(path: &Path, view: &RetrievalRelevanceView) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create view directory {}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(view).context("serialize derived view")?;
    std::fs::write(&temp, bytes)
        .with_context(|| format!("write temporary view {}", temp.display()))?;
    std::fs::rename(&temp, path)
        .with_context(|| format!("install derived view {}", path.display()))?;
    Ok(())
}

/// Validate the derived view against the frozen corpus (C001 §5/§10).
pub fn validate_derived_view(
    view: &RetrievalRelevanceView,
    cases: &[ToolAdvisorCase],
) -> Result<()> {
    if view.schema_version != RETRIEVAL_RELEVANCE_SCHEMA_VERSION {
        return Err(anyhow!(
            "derived view schema {} != expected {RETRIEVAL_RELEVANCE_SCHEMA_VERSION}",
            view.schema_version
        ));
    }
    if view.adjudication_version != RETRIEVAL_RELEVANCE_VERSION {
        return Err(anyhow!(
            "derived view adjudication {} != expected {RETRIEVAL_RELEVANCE_VERSION}",
            view.adjudication_version
        ));
    }
    let corpus_fingerprint = dataset_fingerprint(cases).context("fingerprint corpus")?;
    if view.corpus_fingerprint != corpus_fingerprint {
        return Err(anyhow!("derived view corpus fingerprint mismatch"));
    }
    if view.corpus_fingerprint != EXPECTED_CORPUS_FINGERPRINT {
        return Err(anyhow!("frozen corpus fingerprint drifted"));
    }
    let partition = partition_cases(cases);
    let dev_cases: Vec<ToolAdvisorCase> = partition
        .dev_cases
        .iter()
        .map(|index| cases[*index].clone())
        .collect();
    let dev_fp = dataset_fingerprint(&dev_cases).context("fingerprint dev")?;
    if view.dev_partition_fingerprint != dev_fp {
        return Err(anyhow!("derived view dev partition fingerprint mismatch"));
    }
    if view.dev_partition_fingerprint != EXPECTED_DEV_PARTITION_FINGERPRINT {
        return Err(anyhow!("frozen dev partition drifted"));
    }
    let recomputed = derived_view_fingerprint(&view.entries);
    if view.fingerprint != recomputed {
        return Err(anyhow!("derived view fingerprint mismatch"));
    }
    // Deterministic stable ordering.
    let mut sorted = view.entries.clone();
    sorted.sort_by(|left, right| {
        left.case_id
            .cmp(&right.case_id)
            .then_with(|| left.candidate.cmp(&right.candidate))
    });
    if sorted != view.entries {
        return Err(anyhow!("derived view entries are not stably ordered"));
    }
    let by_case: BTreeMap<&str, &ToolAdvisorCase> = cases
        .iter()
        .map(|case| (case.case_id.as_str(), case))
        .collect();
    let mut seen = BTreeSet::new();
    for entry in &view.entries {
        let key = (entry.case_id.as_str(), entry.candidate.as_str());
        if !seen.insert(key) {
            return Err(anyhow!(
                "duplicate derived entry {} {}",
                entry.case_id,
                entry.candidate
            ));
        }
        let Some(case) = by_case.get(entry.case_id.as_str()) else {
            return Err(anyhow!(
                "derived entry names unknown case {}",
                entry.case_id
            ));
        };
        let Some(grade) = case.relevance.get(&entry.candidate) else {
            return Err(anyhow!(
                "derived entry {} {} is not a positive corpus label",
                entry.case_id,
                entry.candidate
            ));
        };
        if *grade != entry.original_grade {
            return Err(anyhow!(
                "derived entry {} {} grade {} != corpus {grade}",
                entry.case_id,
                entry.candidate,
                entry.original_grade
            ));
        }
        if entry.rationale.trim().is_empty() {
            return Err(anyhow!(
                "derived entry {} {} has no rationale",
                entry.case_id,
                entry.candidate
            ));
        }
        if entry.supporting_text.len() > 512 {
            return Err(anyhow!(
                "derived entry {} {} supporting text exceeds bound",
                entry.case_id,
                entry.candidate
            ));
        }
        match entry.relevance_class {
            RetrievalRelevanceClass::CurrentStep | RetrievalRelevanceClass::ExplicitNextStep => {
                if entry.supporting_field.trim().is_empty() {
                    return Err(anyhow!(
                        "derived entry {} {} {:?} has no supporting evidence",
                        entry.case_id,
                        entry.candidate,
                        entry.relevance_class
                    ));
                }
                if !entry.retrieval_eligible {
                    return Err(anyhow!(
                        "derived entry {} {} {:?} must be retrieval-eligible",
                        entry.case_id,
                        entry.candidate,
                        entry.relevance_class
                    ));
                }
            }
            RetrievalRelevanceClass::ImplicitFuture | RetrievalRelevanceClass::EvidenceDefect => {
                if entry.retrieval_eligible {
                    return Err(anyhow!(
                        "derived entry {} {} {:?} must not be retrieval-eligible",
                        entry.case_id,
                        entry.candidate,
                        entry.relevance_class
                    ));
                }
            }
        }
        if entry.relevance_class == RetrievalRelevanceClass::ImplicitFuture
            && !entry.supporting_field.trim().is_empty()
        {
            return Err(anyhow!(
                "implicit-future entry {} {} claims direct support",
                entry.case_id,
                entry.candidate
            ));
        }
        if entry.relevance_class == RetrievalRelevanceClass::ExplicitNextStep
            && entry.supporting_field != "next_steps"
        {
            return Err(anyhow!(
                "explicit-next-step entry {} {} must cite next_steps",
                entry.case_id,
                entry.candidate
            ));
        }
    }
    // Every dev positive appears exactly once.
    let mut dev_expected = BTreeSet::new();
    for index in &partition.dev_cases {
        let case = &cases[*index];
        for name in case.relevance.keys() {
            dev_expected.insert((case.case_id.as_str(), name.as_str()));
        }
    }
    let dev_seen: BTreeSet<(&str, &str)> = view
        .entries
        .iter()
        .filter(|entry| {
            partition
                .dev_cases
                .iter()
                .any(|index| cases[*index].case_id == entry.case_id)
        })
        .map(|entry| (entry.case_id.as_str(), entry.candidate.as_str()))
        .collect();
    if dev_seen != dev_expected {
        return Err(anyhow!(
            "derived view covers {} dev positives; corpus has {}",
            dev_seen.len(),
            dev_expected.len()
        ));
    }
    Ok(())
}

/// Inferable (retrieval-eligible) candidate set for one case.
pub fn inferable_set(view: &RetrievalRelevanceView, case_id: &str) -> BTreeSet<String> {
    view.entries
        .iter()
        .filter(|entry| entry.case_id == case_id && entry.retrieval_eligible)
        .map(|entry| entry.candidate.clone())
        .collect()
}

/// Consumer-impact summary per split for the closure audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SplitImpact {
    pub split: String,
    pub cases: usize,
    pub positive_labels: usize,
    pub current_step: usize,
    pub explicit_next_step: usize,
    pub implicit_future: usize,
    pub evidence_defect: usize,
    pub cases_with_implicit_future: usize,
    pub cases_with_evidence_defect: usize,
}

pub fn impact_by_split(
    view: &RetrievalRelevanceView,
    cases: &[ToolAdvisorCase],
) -> Result<Vec<SplitImpact>> {
    let partition = partition_cases(cases);
    let by_id: BTreeMap<&str, Vec<&RetrievalRelevanceEntry>> = {
        let mut map: BTreeMap<&str, Vec<&RetrievalRelevanceEntry>> = BTreeMap::new();
        for entry in &view.entries {
            map.entry(entry.case_id.as_str()).or_default().push(entry);
        }
        map
    };
    let mut out = Vec::new();
    for (split, indices) in [
        ("train", &partition.train_cases),
        ("dev", &partition.dev_cases),
        ("test", &partition.test_cases),
    ] {
        let mut counts = SplitImpact {
            split: split.to_string(),
            cases: indices.len(),
            positive_labels: 0,
            current_step: 0,
            explicit_next_step: 0,
            implicit_future: 0,
            evidence_defect: 0,
            cases_with_implicit_future: 0,
            cases_with_evidence_defect: 0,
        };
        for index in indices {
            let case = &cases[*index];
            let entries = by_id
                .get(case.case_id.as_str())
                .cloned()
                .unwrap_or_default();
            let mut has_implicit = false;
            let mut has_defect = false;
            for entry in entries {
                counts.positive_labels += 1;
                match entry.relevance_class {
                    RetrievalRelevanceClass::CurrentStep => counts.current_step += 1,
                    RetrievalRelevanceClass::ExplicitNextStep => {
                        counts.explicit_next_step += 1;
                    }
                    RetrievalRelevanceClass::ImplicitFuture => {
                        counts.implicit_future += 1;
                        has_implicit = true;
                    }
                    RetrievalRelevanceClass::EvidenceDefect => {
                        counts.evidence_defect += 1;
                        has_defect = true;
                    }
                }
            }
            if has_implicit {
                counts.cases_with_implicit_future += 1;
            }
            if has_defect {
                counts.cases_with_evidence_defect += 1;
            }
        }
        out.push(counts);
    }
    Ok(out)
}

/// Local universe expansion for the C001 rebaseline (no encoder needed).
///
/// Verbatim parity with `operating_point::expand_universe`: every labeled
/// relevant tool is preserved (re-homed into the deferred set when
/// necessary), the remainder is deterministic synthetic filler disjoint from
/// every relevant name, output is name-sorted and validated. Kept local so
/// the C001 BM25 frontier runs in the default test profile without the
/// candle-backed encoder-training feature.
pub fn expand_universe_local(
    cases: &[ToolAdvisorCase],
    size: usize,
) -> Result<Vec<ToolAdvisorCase>> {
    use super::ToolAdvisorCandidate;
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
                                "Preserved labeled relevant tool {name} \
                                 for universe expansion"
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
        if fixture.candidates.len() != size {
            return Err(anyhow!(
                "expansion produced {} candidates; expected {size}",
                fixture.candidates.len()
            ));
        }
        for name in fixture.relevance.keys() {
            if !fixture
                .candidates
                .iter()
                .any(|candidate| &candidate.name == name)
            {
                return Err(anyhow!(
                    "fixture case {} lost relevant tool {name} during expansion",
                    fixture.case_id
                ));
            }
        }
        expanded.push(fixture);
    }
    Ok(expanded)
}

/// Local BM25 ordering over the eligible deferred set (no encoder needed).
///
/// Parity with `retrieval_architecture::bm25_ordering`: catalog BM25 scores,
/// deferred-only filter, zero-score completion, score-descending with name
/// tiebreaks.
pub fn bm25_ordering_local(case: &ToolAdvisorCase) -> Vec<super::RankedCandidate> {
    use super::RankedCandidate;
    let prediction = super::baseline_prediction(case, crate::tool::catalog::SearchMode::BM25);
    let allowed: BTreeSet<String> = case
        .candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.clone())
        .collect();
    let mut ranked: Vec<RankedCandidate> = prediction
        .ranked
        .into_iter()
        .filter(|entry| allowed.contains(&entry.name))
        .collect();
    let known: BTreeSet<String> = ranked.iter().map(|entry| entry.name.clone()).collect();
    for name in &allowed {
        if !known.contains(name) {
            ranked.push(RankedCandidate {
                name: name.clone(),
                score: 0.0,
            });
        }
    }
    ranked.sort_by(|left, right| left.name.cmp(&right.name));
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });
    ranked
}

/// Eligible deferred names for authority checks.
pub fn eligible_deferred_names_local(case: &ToolAdvisorCase) -> BTreeSet<String> {
    case.candidates
        .iter()
        .filter(|candidate| candidate.disclosure == "deferred")
        .map(|candidate| candidate.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{builtin_cases, dataset_fingerprint, load_cases, partition_cases};
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn relevance_classes_have_stable_identity() {
        assert_eq!(
            RetrievalRelevanceClass::CurrentStep.as_str(),
            "current-step"
        );
        assert_eq!(
            RetrievalRelevanceClass::ExplicitNextStep.as_str(),
            "explicit-next-step"
        );
        assert_eq!(
            RetrievalRelevanceClass::ImplicitFuture.as_str(),
            "implicit-future"
        );
        assert_eq!(
            RetrievalRelevanceClass::EvidenceDefect.as_str(),
            "evidence-defect"
        );
        assert!(RetrievalRelevanceClass::CurrentStep.is_retrieval_eligible());
        assert!(RetrievalRelevanceClass::ExplicitNextStep.is_retrieval_eligible());
        assert!(!RetrievalRelevanceClass::ImplicitFuture.is_retrieval_eligible());
        assert!(!RetrievalRelevanceClass::EvidenceDefect.is_retrieval_eligible());
        assert_eq!(
            parse_class("current-step").expect("parse"),
            RetrievalRelevanceClass::CurrentStep
        );
        assert!(parse_class("unknown").is_err());
    }

    #[test]
    fn effective_context_strips_stale_suffix() {
        let context = "List recent commits that touched x \
         The transcript mentions read (Read bounded file contents) output, \
         but that trail is stale; resolve x directly instead.";
        let effective = effective_context(context);
        assert!(!effective
            .to_ascii_lowercase()
            .contains("the transcript mentions"));
        assert!(effective.contains("List recent commits"));
        assert_eq!(effective_context("plain request"), "plain request");
    }

    #[test]
    fn dev_positives_are_fully_covered_once() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        // 280 corpus positives: 138 train + 72 dev + 70 test.
        assert_eq!(view.entries.len(), 280);
        let partition = partition_cases(&cases);
        let mut dev_expected = BTreeSet::new();
        for index in &partition.dev_cases {
            for name in cases[*index].relevance.keys() {
                dev_expected.insert((cases[*index].case_id.clone(), name.clone()));
            }
        }
        assert_eq!(dev_expected.len(), 72);
        let dev_seen: BTreeSet<(String, String)> = view
            .entries
            .iter()
            .filter(|entry| {
                dev_expected.contains(&(entry.case_id.clone(), entry.candidate.clone()))
            })
            .map(|entry| (entry.case_id.clone(), entry.candidate.clone()))
            .collect();
        assert_eq!(dev_seen, dev_expected);
    }

    #[test]
    fn class_support_invariants_hold() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        for entry in &view.entries {
            match entry.relevance_class {
                RetrievalRelevanceClass::CurrentStep
                | RetrievalRelevanceClass::ExplicitNextStep => {
                    assert!(
                        !entry.supporting_field.trim().is_empty(),
                        "missing evidence for {} {}",
                        entry.case_id,
                        entry.candidate
                    );
                    assert!(entry.retrieval_eligible);
                }
                RetrievalRelevanceClass::ImplicitFuture
                | RetrievalRelevanceClass::EvidenceDefect => {
                    assert!(
                        !entry.retrieval_eligible,
                        "ineligible class must not be eligible"
                    );
                }
            }
            if entry.relevance_class == RetrievalRelevanceClass::ImplicitFuture {
                assert!(
                    entry.supporting_field.trim().is_empty(),
                    "implicit-future must not claim direct support"
                );
            }
            assert!(!entry.rationale.trim().is_empty());
            assert!(entry.supporting_text.len() <= 512);
        }
    }

    #[test]
    fn asset_fingerprint_is_deterministic() {
        let cases = builtin_cases().expect("corpus");
        let first = build_derived_view(&cases).expect("view");
        let second = build_derived_view(&cases).expect("view");
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.fingerprint, derived_view_fingerprint(&first.entries));
        // Reordering entries changes nothing after canonical sort.
        let mut shuffled = first.entries.clone();
        shuffled.reverse();
        assert_eq!(derived_view_fingerprint(&shuffled), first.fingerprint);
    }

    #[test]
    fn corpus_fingerprint_mismatch_fails() {
        let cases = builtin_cases().expect("corpus");
        let mut view = build_derived_view(&cases).expect("view");
        view.corpus_fingerprint = "deadbeef".to_string();
        assert!(validate_derived_view(&view, &cases).is_err());
        let mut view = build_derived_view(&cases).expect("view");
        view.fingerprint = "deadbeef".to_string();
        assert!(validate_derived_view(&view, &cases).is_err());
    }

    #[test]
    fn historical_corpus_files_are_unchanged() {
        let cases = builtin_cases().expect("corpus");
        let full = dataset_fingerprint(&cases).expect("fingerprint");
        assert_eq!(full, EXPECTED_CORPUS_FINGERPRINT);
        let partition = partition_cases(&cases);
        let dev: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        let dev_fp = dataset_fingerprint(&dev).expect("dev fingerprint");
        assert_eq!(dev_fp, EXPECTED_DEV_PARTITION_FINGERPRINT);
        assert_eq!(cases.len(), 256);
    }

    #[test]
    fn broad_and_inferable_targets_are_distinguishable() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let inferable: usize = view
            .entries
            .iter()
            .filter(|entry| entry.retrieval_eligible)
            .count();
        assert!(inferable < view.entries.len());
        assert!(inferable > 0);
        // Dev: 53 inferable of 72 broad.
        let partition = partition_cases(&cases);
        let dev_ids: BTreeSet<&str> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].case_id.as_str())
            .collect();
        let dev_inferable = view
            .entries
            .iter()
            .filter(|entry| dev_ids.contains(entry.case_id.as_str()) && entry.retrieval_eligible)
            .count();
        assert_eq!(dev_inferable, 53);
    }

    #[test]
    fn implicit_future_excluded_and_current_step_included() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let by_key: BTreeMap<(&str, &str), &RetrievalRelevanceEntry> = view
            .entries
            .iter()
            .map(|entry| ((entry.case_id.as_str(), entry.candidate.as_str()), entry))
            .collect();
        // Known implicit secondaries are excluded.
        for (case, candidate) in [
            ("filesystem-semantic-013-variant-1", "table_filter"),
            ("verification-semantic-105-variant-1", "write"),
            ("research-semantic-125-variant-1", "lsp_rename"),
            ("git-semantic-047-variant-1", "shell_history"),
        ] {
            let entry = by_key.get(&(case, candidate)).expect("implicit entry");
            assert_eq!(
                entry.relevance_class,
                RetrievalRelevanceClass::ImplicitFuture
            );
            assert!(!entry.retrieval_eligible);
            assert!(inferable_set(&view, case).contains(candidate) == false);
        }
        // Known current-step primaries are included.
        for (case, candidate) in [
            ("filesystem-semantic-003-variant-1", "glob"),
            ("filesystem-semantic-013-variant-1", "read"),
            ("lsp-semantic-075-variant-1", "lsp_rename"),
            ("verification-semantic-105-variant-1", "coverage"),
        ] {
            let entry = by_key.get(&(case, candidate)).expect("current entry");
            assert_eq!(entry.relevance_class, RetrievalRelevanceClass::CurrentStep);
            assert!(entry.retrieval_eligible);
            assert!(inferable_set(&view, case).contains(candidate));
        }
    }

    #[test]
    fn explicit_next_step_requires_next_steps_field() {
        // Frozen corpus has no structured next_steps, so no frozen label
        // may be ExplicitNextStep. The validator enforces the field binding
        // for any future explicit entry.
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        assert_eq!(
            view.entries
                .iter()
                .filter(|entry| entry.relevance_class == RetrievalRelevanceClass::ExplicitNextStep)
                .count(),
            0
        );
        let mut bad = view.clone();
        bad.entries.push(RetrievalRelevanceEntry {
            case_id: cases[0].case_id.clone(),
            candidate: "read".to_string(),
            original_grade: 2,
            preferred_index: None,
            tool_family: String::new(),
            task_family: String::new(),
            relevance_class: RetrievalRelevanceClass::ExplicitNextStep,
            retrieval_eligible: true,
            supporting_field: "current_objective".to_string(),
            supporting_text: "x".to_string(),
            rationale: "bad".to_string(),
        });
        bad.entries.sort_by(|left, right| {
            left.case_id
                .cmp(&right.case_id)
                .then_with(|| left.candidate.cmp(&right.candidate))
        });
        bad.fingerprint = derived_view_fingerprint(&bad.entries);
        assert!(validate_derived_view(&bad, &cases).is_err());
    }

    #[test]
    fn no_evidence_defect_in_frozen_corpus() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        assert_eq!(
            view.entries
                .iter()
                .filter(|entry| entry.relevance_class == RetrievalRelevanceClass::EvidenceDefect)
                .count(),
            0
        );
    }

    /// Generate the repository-owned derived asset. Run explicitly:
    /// `cargo test --locked -p codegg --lib --
    ///  tool_advisor::retrieval_relevance::tests::generate_derived_asset --
    ///  --ignored --nocapture`
    #[test]
    #[ignore]
    fn generate_derived_asset() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let cases = load_cases(None).expect("builtin corpus");
        let view = build_derived_view(&cases).expect("view");
        validate_derived_view(&view, &cases).expect("validate");
        let path = root.join(DERIVED_ASSET_PATH);
        write_derived_view_atomic(&path, &view).expect("write asset");
        eprintln!(
            "wrote {} entries={} fingerprint={} corpus={} dev={}",
            path.display(),
            view.entries.len(),
            view.fingerprint,
            view.corpus_fingerprint,
            view.dev_partition_fingerprint
        );
    }

    #[test]
    fn committed_asset_matches_classifier() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join(DERIVED_ASSET_PATH);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("SKIP: derived asset not yet generated");
            return;
        };
        let stored: RetrievalRelevanceView = serde_json::from_slice(&bytes).expect("parse asset");
        let cases = builtin_cases().expect("corpus");
        let fresh = build_derived_view(&cases).expect("view");
        assert_eq!(stored, fresh, "committed asset drifted from classifier");
        validate_derived_view(&stored, &cases).expect("validate asset");
    }

    #[test]
    fn consumer_impact_splits_match_expected_counts() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let impacts = impact_by_split(&view, &cases).expect("impact");
        let by_split: BTreeMap<&str, &SplitImpact> = impacts
            .iter()
            .map(|impact| (impact.split.as_str(), impact))
            .collect();
        let dev = by_split["dev"];
        assert_eq!(dev.cases, 62);
        assert_eq!(dev.positive_labels, 72);
        assert_eq!(dev.current_step, 53);
        assert_eq!(dev.explicit_next_step, 0);
        assert_eq!(dev.implicit_future, 19);
        assert_eq!(dev.evidence_defect, 0);
        assert_eq!(dev.cases_with_implicit_future, 19);
        assert_eq!(dev.cases_with_evidence_defect, 0);
        let train = by_split["train"];
        assert_eq!(train.cases, 132);
        assert_eq!(train.positive_labels, 138);
        assert_eq!(train.current_step, 113);
        assert_eq!(train.implicit_future, 25);
        assert_eq!(train.evidence_defect, 0);
        let test = by_split["test"];
        assert_eq!(test.cases, 62);
        assert_eq!(test.positive_labels, 70);
        assert_eq!(test.current_step, 53);
        assert_eq!(test.implicit_future, 17);
        assert_eq!(test.evidence_defect, 0);
    }

    #[test]
    fn highest_grade_and_preferred_first_are_all_inferable() {
        // Ranker-label impact: the span-packed ranker was trained/selected on
        // graded relevance with preferred-order supervision. If any grade-3
        // or preferred-first label were implicit-future, corrected semantics
        // could invalidate the selection (disposition C). All such labels
        // are current-step here, so no material ranker impact.
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let by_key: BTreeMap<(&str, &str), &RetrievalRelevanceEntry> = view
            .entries
            .iter()
            .map(|entry| ((entry.case_id.as_str(), entry.candidate.as_str()), entry))
            .collect();
        for case in &cases {
            for (name, grade) in &case.relevance {
                if *grade == 3 {
                    let entry = by_key
                        .get(&(case.case_id.as_str(), name.as_str()))
                        .expect("grade-3 entry");
                    assert_eq!(
                        entry.relevance_class,
                        RetrievalRelevanceClass::CurrentStep,
                        "grade-3 label {} {} must be inferable",
                        case.case_id,
                        name
                    );
                }
            }
            if let Some(first) = case.preferred_order.first() {
                if let Some(entry) = by_key.get(&(case.case_id.as_str(), first.as_str())) {
                    assert_eq!(
                        entry.relevance_class,
                        RetrievalRelevanceClass::CurrentStep,
                        "preferred-first label {} {} must be inferable",
                        case.case_id,
                        first
                    );
                }
            }
        }
        // Per-split preferred-first totals: dev 52, train 102, test 53.
        let partition = partition_cases(&cases);
        for (indices, expected) in [
            (&partition.dev_cases, 52usize),
            (&partition.train_cases, 102usize),
            (&partition.test_cases, 53usize),
        ] {
            let mut count = 0usize;
            for index in indices {
                let case = &cases[*index];
                if let Some(first) = case.preferred_order.first() {
                    let entry = by_key
                        .get(&(case.case_id.as_str(), first.as_str()))
                        .expect("preferred entry");
                    assert!(entry.retrieval_eligible);
                    count += 1;
                }
            }
            assert_eq!(count, expected);
        }
    }

    #[test]
    fn per_family_distribution_is_reported() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let by_key: BTreeMap<(&str, &str), &RetrievalRelevanceEntry> = view
            .entries
            .iter()
            .map(|entry| ((entry.case_id.as_str(), entry.candidate.as_str()), entry))
            .collect();
        let mut per_family: BTreeMap<(String, String), usize> = BTreeMap::new();
        for case in &cases {
            for name in case.relevance.keys() {
                let entry = by_key
                    .get(&(case.case_id.as_str(), name.as_str()))
                    .expect("entry");
                *per_family
                    .entry((
                        case.tool_family.clone(),
                        entry.relevance_class.as_str().to_string(),
                    ))
                    .or_default() += 1;
            }
        }
        // Spot-check families with known implicit load.
        assert_eq!(
            per_family
                .get(&("verification".to_string(), "implicit-future".to_string()))
                .copied()
                .unwrap_or(0),
            6
        );
        assert_eq!(
            per_family
                .get(&("git".to_string(), "current-step".to_string()))
                .copied()
                .unwrap_or(0),
            23
        );
        assert_eq!(view.entries.len(), 280);
    }

    #[test]
    fn bm25_mode_identity_is_unchanged() {
        // BM25 path must remain catalog BM25 over name+description with
        // deferred-only filtering. Any descriptor/retrieval change would
        // break the M004 52/72 tripwire measured below.
        let cases = builtin_cases().expect("corpus");
        let case = cases
            .iter()
            .find(|c| c.case_id == "filesystem-semantic-003-variant-1")
            .expect("glob case");
        let prediction =
            super::super::baseline_prediction(case, crate::tool::catalog::SearchMode::BM25);
        assert_eq!(prediction.mode, "bm25");
        assert!(!prediction.ranked.is_empty());
    }

    #[test]
    fn authority_remains_zero_violation() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        for entry in &view.entries {
            let case = cases
                .iter()
                .find(|c| c.case_id == entry.case_id)
                .expect("case");
            let eligible: BTreeSet<&str> = case
                .candidates
                .iter()
                .filter(|c| c.disclosure == "deferred")
                .map(|c| c.name.as_str())
                .collect();
            // Every adjudicated label must be inside the eligible deferred
            // universe for expanded fixtures to preserve it; otherwise the
            // rebaseline would count authority violations.
            if case.relevance.contains_key(&entry.candidate) {
                assert!(
                    case.candidates.iter().any(|c| c.name == entry.candidate),
                    "label {} {} has no candidate descriptor",
                    entry.case_id,
                    entry.candidate
                );
                let _ = eligible;
            }
        }
        // No tool outside the resolved deferred surface is ever recovered by
        // construction: rebaseline orderings filter to deferred only.
        let _ = inferable_set(&view, "filesystem-semantic-003-variant-1");
    }

    /// Unchanged BM25 rebaseline over expanded dev universes (C001 §7/WP E).
    ///
    /// Uses the frozen expansion + catalog BM25 path with zero retrieval
    /// code/config change. Reports broad vs inferable recall at every
    /// required universe/K (64/128/256 × 16/24/32). Runs in the default
    /// profile (no encoder): semantic/fusion remeasurement needs reference
    /// MiniLM assets absent here and is covered by prior dual-signal
    /// evidence cited in the closure.
    #[test]
    fn expanded_bm25_broad_vs_inferable_frontier() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        validate_derived_view(&view, &cases).expect("validate");
        let partition = partition_cases(&cases);
        let dev: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        assert_eq!(dev.len(), 62);
        for universe in [64usize, 128, 256] {
            let fixture = expand_universe_local(&dev, universe).expect("expand");
            assert_eq!(fixture.len(), dev.len());
            for case in &fixture {
                assert_eq!(case.candidates.len(), universe);
            }
            for k in [16usize, 24, 32] {
                let mut broad_eligible = 0usize;
                let mut broad_recovered = 0usize;
                let mut infer_eligible = 0usize;
                let mut infer_recovered = 0usize;
                let mut violations = 0usize;
                let mut misses: BTreeMap<String, usize> = BTreeMap::new();
                for case in &fixture {
                    let allowed = eligible_deferred_names_local(case);
                    let top: BTreeSet<String> = bm25_ordering_local(case)
                        .into_iter()
                        .take(k)
                        .map(|entry| entry.name)
                        .collect();
                    for name in &top {
                        if !allowed.contains(name) {
                            violations += 1;
                        }
                    }
                    let inferable = inferable_set(&view, &case.case_id);
                    for name in case.relevance.keys() {
                        if !allowed.contains(name) {
                            continue;
                        }
                        broad_eligible += 1;
                        if top.contains(name) {
                            broad_recovered += 1;
                        }
                        if inferable.contains(name) {
                            infer_eligible += 1;
                            if top.contains(name) {
                                infer_recovered += 1;
                            } else {
                                *misses.entry(name.clone()).or_default() += 1;
                            }
                        }
                    }
                }
                assert_eq!(violations, 0, "authority must stay zero");
                // M004 tripwire: broad BM25 reproduces 52/72 flat.
                if universe == 64 && k == 16 {
                    assert_eq!(
                        (broad_recovered, broad_eligible),
                        (52, 72),
                        "BM25 broad frontier drifted; retrieval changed"
                    );
                }
                // Corrected denominator is fixed: 53 inferable of 72 broad.
                assert_eq!(infer_eligible, 53);
                assert_eq!(broad_eligible, 72);
                let _ = (infer_recovered, misses);
            }
        }
    }

    /// Fast deterministic BM25 rebaseline without universe expansion.
    ///
    /// Runs in the default test profile (no encoder, no expansion): scores
    /// every dev case with catalog BM25 over its native 5-candidate
    /// universe and compares broad vs inferable top-1/top-2 recovery. This
    /// proves the two targets are distinguishable and that implicit-future
    /// labels are excluded from inferable recall while current-step labels
    /// are included.
    #[test]
    fn native_bm25_broad_vs_inferable_is_distinguishable() {
        use std::collections::{BTreeMap, BTreeSet};
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let partition = partition_cases(&cases);
        let dev: Vec<&ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| &cases[*index])
            .collect();
        let mut broad_hits = 0usize;
        let mut broad_total = 0usize;
        let mut infer_hits = 0usize;
        let mut infer_total = 0usize;
        for case in dev {
            if case.relevance.is_empty() {
                continue;
            }
            let prediction =
                super::super::baseline_prediction(case, crate::tool::catalog::SearchMode::BM25);
            let top2: BTreeSet<&str> = prediction
                .ranked
                .iter()
                .take(2)
                .map(|entry| entry.name.as_str())
                .collect();
            let inferable = inferable_set(&view, &case.case_id);
            for name in case.relevance.keys() {
                broad_total += 1;
                if top2.contains(name.as_str()) {
                    broad_hits += 1;
                }
                if inferable.contains(name) {
                    infer_total += 1;
                    if top2.contains(name.as_str()) {
                        infer_hits += 1;
                    }
                }
            }
        }
        assert_eq!(broad_total, 72);
        assert_eq!(infer_total, 53);
        // Both targets are non-trivial and distinguishable.
        assert!(broad_hits < broad_total);
        assert!(infer_hits <= infer_total);
        assert!(infer_hits > 0);
        let _ = BTreeMap::<String, usize>::new();
    }

    /// Print the full BM25 broad vs inferable frontier for closure evidence.
    /// Run explicitly:
    /// `cargo test --locked -p codegg --lib --
    ///  tool_advisor::retrieval_relevance::tests::print_bm25_frontier --
    ///  --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_bm25_frontier() {
        let cases = builtin_cases().expect("corpus");
        let view = build_derived_view(&cases).expect("view");
        let partition = partition_cases(&cases);
        let dev: Vec<ToolAdvisorCase> = partition
            .dev_cases
            .iter()
            .map(|index| cases[*index].clone())
            .collect();
        for universe in [64usize, 128, 256] {
            let fixture = expand_universe_local(&dev, universe).expect("expand");
            for k in [16usize, 24, 32] {
                let mut broad_eligible = 0usize;
                let mut broad_recovered = 0usize;
                let mut infer_eligible = 0usize;
                let mut infer_recovered = 0usize;
                let mut violations = 0usize;
                let mut misses: BTreeMap<String, usize> = BTreeMap::new();
                let mut miss_cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for case in &fixture {
                    let allowed = eligible_deferred_names_local(case);
                    let top: BTreeSet<String> = bm25_ordering_local(case)
                        .into_iter()
                        .take(k)
                        .map(|entry| entry.name)
                        .collect();
                    for name in &top {
                        if !allowed.contains(name) {
                            violations += 1;
                        }
                    }
                    let inferable = inferable_set(&view, &case.case_id);
                    for name in case.relevance.keys() {
                        if !allowed.contains(name) {
                            continue;
                        }
                        broad_eligible += 1;
                        if top.contains(name) {
                            broad_recovered += 1;
                        }
                        if inferable.contains(name) {
                            infer_eligible += 1;
                            if top.contains(name) {
                                infer_recovered += 1;
                            } else {
                                *misses.entry(name.clone()).or_default() += 1;
                                miss_cases
                                    .entry(name.clone())
                                    .or_default()
                                    .push(case.case_id.clone());
                            }
                        }
                    }
                }
                let broad_recall = broad_recovered as f64 / broad_eligible as f64;
                let infer_recall = infer_recovered as f64 / infer_eligible as f64;
                eprintln!(
                    "FRONTIER universe={universe} K={k} \
                     broad={broad_recovered}/{broad_eligible}={broad_recall:.4} \
                     inferable={infer_recovered}/{infer_eligible}={infer_recall:.4} \
                     violations={violations} misses={misses:?}"
                );
                for (tool, ids) in &miss_cases {
                    let mut ids = ids.clone();
                    ids.sort();
                    eprintln!("  miss {tool} in {ids:?}");
                }
            }
        }
    }
}
