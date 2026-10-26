use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use walkdir::WalkDir;

const SKIP_DIRS: &[&str] = &[".git", "target", ".codegg", "node_modules", ".svn"];
const MAX_FILES_SCANNED: usize = 2000;
const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB
/// Per-file scan cap: only the leading bytes are lowercased and matched,
/// so a 10MB log cannot dominate a run. Mirrors the bounded-scan contract.
const MAX_SCAN_CHARS: usize = 32_768;
/// Repo-how seeds answered directly by these files. Checked explicitly
/// before the walk so walk-order truncation can never hide them.
const SEED_FILES: &[&str] = &["scripts/verify.sh", "AGENTS.md", "architecture/testing.md"];

/// Tokenize like `crate::tool::catalog` (split on `/._-` and every other
/// non-alphanumeric): lowercase alphanumeric terms. `scripts/verify.sh`
/// becomes `scripts` + `verify` instead of one never-matching literal.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub struct LocalRepoSource {
    project_root: PathBuf,
}

impl LocalRepoSource {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    async fn collect_file(&self, path: &str) -> Result<SourceRecord> {
        let resolved = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.project_root.join(path)
        };

        let metadata = tokio::fs::metadata(&resolved).await.map_err(|e| {
            ResearchError::SourceCollection(format!(
                "failed to read metadata for {}: {}",
                resolved.display(),
                e
            ))
        })?;

        let size = metadata.len() as usize;
        if size > MAX_FILE_SIZE {
            return Err(ResearchError::FileTooLarge {
                path: resolved.display().to_string(),
                size,
                max: MAX_FILE_SIZE,
            });
        }

        let content = tokio::fs::read(&resolved)
            .await
            .map_err(ResearchError::Io)?;
        let content_hash = format!("{:x}", Sha256::digest(&content));

        let source_type = SourceType::LocalFile;
        let source_quality = match resolved.extension().and_then(|e| e.to_str()) {
            Some("rs") => SourceQuality::SourceCode,
            Some("toml") | Some("json") | Some("yaml") | Some("yml") => SourceQuality::Primary,
            _ => SourceQuality::Secondary,
        };

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: resolved.display().to_string(),
            title: resolved
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string()),
            source_type,
            source_quality,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: Some(content_hash),
            locator: SourceLocator::FileRange {
                path: resolved,
                start_line: 1,
                end_line: content.iter().filter(|&&b| b == b'\n').count() + 1,
            },
            notes: Vec::new(),
        })
    }

    fn extract_keywords(question: &str) -> Vec<String> {
        tokenize(question)
            .into_iter()
            .filter(|w| w.len() >= 3)
            .collect()
    }

    /// Filename-weighted keyword score mirroring `catalog::keyword_score`:
    /// a path-token hit (exact or prefix) outweighs a content hit 10:1,
    /// with a small bonus when both agree. Returns `(score, matched)` so
    /// callers can rank and label deterministically.
    fn score_file(
        query_terms: &[String],
        relative_path: &str,
        content_lower: &str,
    ) -> Option<(usize, Vec<String>)> {
        let path_tokens = tokenize(relative_path);
        let mut score = 0usize;
        let mut matched = Vec::new();
        for term in query_terms {
            let path_match = path_tokens
                .iter()
                .any(|candidate| candidate == term || candidate.starts_with(term.as_str()));
            let content_match = content_lower.contains(term.as_str());
            if path_match || content_match {
                matched.push(term.clone());
                if path_match {
                    score += 10;
                } else {
                    score += 1;
                }
                if path_match && content_match {
                    score += 2;
                }
            }
        }
        if matched.is_empty() {
            None
        } else {
            Some((score, matched))
        }
    }

    async fn score_path(
        &self,
        path: &Path,
        keywords: &[String],
    ) -> Option<(usize, Vec<String>, Vec<u8>)> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        let size = metadata.len() as usize;
        if size > MAX_FILE_SIZE || size == 0 {
            return None;
        }
        let content = tokio::fs::read(path).await.ok()?;
        // Chunk+cap: match only the leading window instead of lowercasing
        // megabytes per file.
        let text = String::from_utf8_lossy(&content);
        let capped: String = text.chars().take(MAX_SCAN_CHARS).collect();
        let content_lower = capped.to_lowercase();
        let relative = path
            .strip_prefix(&self.project_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        let (score, matched) = Self::score_file(keywords, &relative, &content_lower)?;
        Some((score, matched, content))
    }

    async fn search_local(
        &self,
        request: &ResearchRequest,
        _plan: &ResearchPlan,
    ) -> Result<Vec<SourceRecord>> {
        let keywords = Self::extract_keywords(&request.question);
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Ranked candidates: (score, path-string, matched, content bytes).
        let mut ranked: Vec<(usize, String, Vec<String>, Vec<u8>)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Seed repo-how files first so walk-order truncation can never hide
        // `scripts/verify.sh`, `AGENTS.md`, or `architecture/testing.md`
        // for verify/testing queries.
        for seed in SEED_FILES {
            let path = self.project_root.join(seed);
            if let Some((score, matched, content)) = self.score_path(&path, &keywords).await {
                let key = path.display().to_string();
                if seen.insert(key.clone()) {
                    ranked.push((score, key, matched, content));
                }
            }
        }

        let mut files_scanned = 0usize;

        for entry in WalkDir::new(&self.project_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.file_type().is_dir() || !SKIP_DIRS.contains(&e.file_name().to_str().unwrap_or(""))
            })
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            files_scanned += 1;
            if files_scanned > MAX_FILES_SCANNED {
                break;
            }

            let path = entry.path();
            let key = path.display().to_string();
            if seen.contains(&key) {
                continue;
            }
            if let Some((score, matched, content)) = self.score_path(path, &keywords).await {
                seen.insert(key.clone());
                ranked.push((score, key, matched, content));
            }
        }

        // Highest score first; path ascending keeps ties deterministic.
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

        let mut sources = Vec::with_capacity(ranked.len());
        for (score, uri, matched_keywords, content) in ranked {
            let path = Path::new(&uri);
            let content_hash = format!("{:x}", Sha256::digest(&content));

            let source_quality = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => SourceQuality::SourceCode,
                Some("toml") | Some("json") | Some("yaml") | Some("yml") => SourceQuality::Primary,
                _ => SourceQuality::Secondary,
            };

            let _ = score;
            // NOTE (out-of-scope unblock 2026-10-25): `path` borrowed `uri`
            // while `uri` was moved into the record below, which never
            // compiled. Materialize the owned title first so the move is legal.
            // No behavior change; fixes a pre-existing E0505 outside the
            // agentfindings F9–F15 scope.
            let title = Path::new(&uri)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            sources.push(SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri,
                title,
                source_type: SourceType::LocalSearchResult,
                source_quality,
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: Some(content_hash),
                locator: SourceLocator::TextSpan {
                    label: format!("matched: {}", matched_keywords.join(", ")),
                },
                notes: vec![format!("matched keywords: {}", matched_keywords.join(", "))],
            });
        }

        Ok(sources)
    }
}

impl ResearchSourceAdapter for LocalRepoSource {
    fn name(&self) -> &'static str {
        "local_repo"
    }

    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>> {
        Box::pin(async move {
            let mut sources = Vec::new();

            for source_spec in &request.sources {
                match source_spec.spec_type {
                    SourceSpecType::File => {
                        sources.push(self.collect_file(&source_spec.value).await?);
                    }
                    SourceSpecType::Local => {
                        sources.extend(self.search_local(request, plan).await?);
                    }
                    _ => {}
                }
            }

            let max = request.budget.max_sources.min(sources.len());
            sources.truncate(max);
            Ok(sources)
        })
    }

    fn collect_with_receipt<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<SourceCollection>> + Send + 'a>> {
        Box::pin(async move {
            match self.collect(request, plan).await {
                Ok(sources) => {
                    let links = sources
                        .iter()
                        .map(|source| RetrievalSourceLink {
                            source_id: source.id.clone(),
                            upstream_stable_id: None,
                            group: None,
                            providers: Vec::new(),
                            published_at: source.published_at,
                            provenance: None,
                        })
                        .collect::<Vec<_>>();
                    let count = links.len();
                    Ok(SourceCollection {
                        sources,
                        receipts: vec![ResearchRetrievalReceipt {
                            adapter: "local_repo".to_string(),
                            state: RetrievalState::Complete,
                            source_count: count,
                            coverage: Some("local workspace search".to_string()),
                            warning_codes: Vec::new(),
                            trust_markers: Vec::new(),
                            failed_providers: Vec::new(),
                            sources: links,
                        }],
                    })
                }
                Err(e) => Ok(SourceCollection {
                    sources: Vec::new(),
                    receipts: vec![ResearchRetrievalReceipt {
                        adapter: "local_repo".to_string(),
                        state: RetrievalState::Failed,
                        source_count: 0,
                        coverage: Some(format!("local collection failed: {e}")),
                        warning_codes: vec!["local_collection_failed".to_string()],
                        trust_markers: Vec::new(),
                        failed_providers: Vec::new(),
                        sources: Vec::new(),
                    }],
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::sources::ResearchSourceAdapter;

    fn test_request(question: &str) -> ResearchRequest {
        ResearchRequest {
            id: "run-1".to_string(),
            question: question.to_string(),
            mode: ResearchMode::NarrowAnswer,
            audience: ResearchAudience::Human,
            depth: ResearchDepth::Low,
            output_profiles: vec![],
            constraints: vec![],
            sources: vec![ResearchSourceSpec {
                spec_type: SourceSpecType::Local,
                value: String::new(),
            }],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 8,
                max_chunks_per_source: 20,
                max_evidence_spans: 200,
                max_model_calls: 0,
                max_output_tokens: None,
                allow_network: false,
            },
            created_at: chrono::Utc::now(),
        }
    }

    fn test_plan() -> ResearchPlan {
        ResearchPlan {
            scope: "test".to_string(),
            comparison_axes: vec![],
            source_classes: vec![],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        }
    }

    #[tokio::test]
    async fn local_collection_emits_a_complete_receipt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "research probe keyword alpha").unwrap();
        let source = LocalRepoSource::new(dir.path().to_path_buf());
        let request = test_request("probe keyword alpha");
        let collection = source
            .collect_with_receipt(&request, &test_plan())
            .await
            .unwrap();
        assert_eq!(collection.receipts.len(), 1);
        let receipt = &collection.receipts[0];
        assert_eq!(receipt.adapter, "local_repo");
        assert_eq!(receipt.state, RetrievalState::Complete);
        assert_eq!(receipt.source_count, collection.sources.len());
        assert!(!collection.sources.is_empty());
    }

    #[tokio::test]
    async fn local_receipt_is_complete_even_with_no_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "unrelated content").unwrap();
        let source = LocalRepoSource::new(dir.path().to_path_buf());
        let request = test_request("zzz-no-such-keyword-qqq");
        let collection = source
            .collect_with_receipt(&request, &test_plan())
            .await
            .unwrap();
        assert_eq!(collection.receipts.len(), 1);
        assert_eq!(collection.receipts[0].adapter, "local_repo");
        assert_eq!(collection.receipts[0].state, RetrievalState::Complete);
        assert_eq!(collection.sources.len(), 0);
    }

    #[test]
    fn tokenize_splits_path_punctuation_like_tool_catalog() {
        // Regression: `scripts/verify.sh` was one whitespace token and never
        // matched file content; catalog-style splitting yields `verify`.
        let tokens = tokenize("How does scripts/verify.sh select quick?");
        assert!(tokens.contains(&"scripts".to_string()));
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"quick".to_string()));
        assert!(!tokens.iter().any(|t| t.contains('/')));
    }

    #[test]
    fn path_token_hit_outranks_content_only_hit() {
        let terms = vec!["verify".to_string(), "quick".to_string()];
        let path_hit = LocalRepoSource::score_file(&terms, "scripts/verify.sh", "quick full");
        let content_hit =
            LocalRepoSource::score_file(&terms, "research/tui-notes.md", "quick full verify");
        let (path_score, _) = path_hit.expect("path file must match");
        let (content_score, _) = content_hit.expect("content file must match");
        assert!(
            path_score > content_score,
            "filename-weighted path hit ({path_score}) must beat content-only ({content_score})"
        );
    }

    #[tokio::test]
    async fn verify_how_query_surfaces_verify_sh_first() {
        // End-to-end shape of F3: a repo-how question must rank the seeded
        // `scripts/verify.sh` above generic `research/*.md` notes.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::create_dir_all(dir.path().join("research")).unwrap();
        std::fs::write(
            dir.path().join("scripts/verify.sh"),
            "run_quick run_full run_scoped quick vs full vs scoped selection",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("research/tui-notes.md"),
            "tui linux notes with quick mention",
        )
        .unwrap();
        let source = LocalRepoSource::new(dir.path().to_path_buf());
        let request =
            test_request("How does scripts/verify.sh select quick vs full vs scoped verification?");
        let collection = source
            .collect_with_receipt(&request, &test_plan())
            .await
            .unwrap();
        assert!(!collection.sources.is_empty());
        let first = &collection.sources[0].uri;
        assert!(
            first.ends_with("scripts/verify.sh"),
            "verify-how query must surface scripts/verify.sh first, got: {first}"
        );
    }
}
