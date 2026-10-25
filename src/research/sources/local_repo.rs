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
const MAX_FILES_SCANNED: usize = 50;
const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB

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
        question
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 3)
            .collect()
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

        let mut sources = Vec::new();
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
            let metadata = match tokio::fs::metadata(path).await {
                Ok(m) => m,
                Err(_) => continue,
            };

            let size = metadata.len() as usize;
            if size > MAX_FILE_SIZE || size == 0 {
                continue;
            }

            let content = match tokio::fs::read(path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let content_lower = String::from_utf8_lossy(&content).to_lowercase();
            let matched_keywords: Vec<&str> = keywords
                .iter()
                .filter(|k| content_lower.contains(k.as_str()))
                .map(|k| k.as_str())
                .collect();

            if matched_keywords.is_empty() {
                continue;
            }

            let content_hash = format!("{:x}", Sha256::digest(&content));

            let source_quality = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => SourceQuality::SourceCode,
                Some("toml") | Some("json") | Some("yaml") | Some("yml") => SourceQuality::Primary,
                _ => SourceQuality::Secondary,
            };

            sources.push(SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri: path.display().to_string(),
                title: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string()),
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
}
