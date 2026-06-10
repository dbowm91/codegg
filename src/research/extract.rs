use chrono::Utc;

use crate::provider::Provider;
use crate::research::llm;
use crate::research::templates;
use crate::research::types::*;

/// Chunk a source's content into evidence spans deterministically.
/// For local files: chunk by line windows (100 lines with 10-line overlap).
/// For URL text: chunk by heading breaks or by ~2000 character windows.
pub fn chunk_source_content(
    source: &SourceRecord,
    content: &str,
    max_chunks: usize,
) -> Vec<(String, SourceLocator)> {
    match &source.source_type {
        SourceType::LocalFile | SourceType::LocalSearchResult => {
            chunk_local_file(source, content, max_chunks)
        }
        SourceType::Url | SourceType::HtmlPage | SourceType::MarkdownPage => {
            chunk_url_text(source, content, max_chunks)
        }
        _ => chunk_url_text(source, content, max_chunks),
    }
}

/// Chunk a local file by line windows.
/// Returns windows of 100 lines with 10-line overlap.
/// Line numbers in SourceLocator are 1-indexed.
fn chunk_local_file(
    source: &SourceRecord,
    content: &str,
    max_chunks: usize,
) -> Vec<(String, SourceLocator)> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let window_size: usize = 100;
    let overlap: usize = 10;
    let step = window_size.saturating_sub(overlap);
    let mut chunks = Vec::new();

    let mut start = 0;
    while start < lines.len() && chunks.len() < max_chunks {
        let end = (start + window_size).min(lines.len());

        // Skip chunks that are entirely blank
        let chunk_text = lines[start..end].join("\n");
        if chunk_text.trim().is_empty() {
            start += step;
            continue;
        }

        // Convert to 1-indexed line numbers
        let start_line = start + 1;
        let end_line = end;

        let path = source
            .locator
            .as_file_path()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(&source.uri));

        let locator = SourceLocator::FileRange {
            path,
            start_line,
            end_line,
        };

        chunks.push((chunk_text, locator));
        start += step;
    }

    chunks
}

/// Chunk URL text by heading breaks or ~2000 character windows on word boundaries.
fn chunk_url_text(
    _source: &SourceRecord,
    content: &str,
    max_chunks: usize,
) -> Vec<(String, SourceLocator)> {
    let max_chars = 2000;
    let mut chunks = Vec::new();

    // Try splitting on heading lines (lines starting with #)
    let heading_positions: Vec<usize> = content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    // Also collect double-newline positions as paragraph breaks
    let bytes = content.as_bytes();
    let mut paragraph_breaks: Vec<usize> = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            // Find the character offset of the start of the second newline
            let mut char_offset = 0;
            for (ci, _) in content.char_indices() {
                if ci >= i {
                    char_offset = ci;
                    break;
                }
            }
            paragraph_breaks.push(char_offset);
            i += 2;
        } else {
            i += 1;
        }
    }

    // Merge heading and paragraph positions, sort, deduplicate
    let mut break_positions: Vec<usize> = heading_positions
        .iter()
        .filter_map(|&line_idx| {
            // Convert line index to char offset
            content
                .lines()
                .enumerate()
                .find(|&(i, _)| i == line_idx)
                .map(|(_i, line)| {
                    // Find the byte offset of this line
                    let mut offset = 0;
                    for (ci, _) in content.char_indices() {
                        let remaining = &content[ci..];
                        if remaining.starts_with(line) {
                            offset = ci;
                            break;
                        }
                    }
                    offset
                })
        })
        .chain(paragraph_breaks.iter().copied())
        .collect();

    break_positions.sort_unstable();
    break_positions.dedup();

    if !break_positions.is_empty() && break_positions.len() <= max_chunks {
        // Split on heading/paragraph breaks
        let mut prev = 0;
        for &pos in &break_positions {
            if chunks.len() >= max_chunks {
                break;
            }
            let chunk_text = content[prev..pos].trim();
            if !chunk_text.is_empty() {
                let label = format!("chars {}-{}", prev, pos);
                chunks.push((chunk_text.to_string(), SourceLocator::TextSpan { label }));
            }
            prev = pos;
        }
        // Final chunk
        if chunks.len() < max_chunks {
            let remaining = content[prev..].trim();
            if !remaining.is_empty() {
                let label = format!("chars {}-{}", prev, content.len());
                chunks.push((remaining.to_string(), SourceLocator::TextSpan { label }));
            }
        }
    } else {
        // Fallback: split at ~2000 character boundaries on word boundaries
        let mut remaining = content;
        let mut offset = 0;
        while !remaining.is_empty() && chunks.len() < max_chunks {
            let split_at = if remaining.len() <= max_chars {
                remaining.len()
            } else {
                // Find a word boundary near max_chars
                let candidate = max_chars.min(remaining.len());
                // Look back from candidate for whitespace
                let mut boundary = candidate;
                while boundary > 0 && !remaining.as_bytes()[boundary].is_ascii_whitespace() {
                    boundary -= 1;
                }
                if boundary == 0 {
                    // No word boundary found, hard split at max_chars
                    candidate
                } else {
                    boundary
                }
            };

            let chunk_text = remaining[..split_at].trim().to_string();
            if !chunk_text.is_empty() {
                let label = format!("chars {}-{}", offset, offset + split_at);
                chunks.push((chunk_text, SourceLocator::TextSpan { label }));
            }

            remaining = &remaining[split_at..];
            offset += split_at;
        }
    }

    chunks
}

/// Create deterministic evidence spans from chunks (without model).
/// Each chunk becomes an EvidenceSpan with the text as-is.
pub fn deterministic_evidence(
    run_id: &str,
    source: &SourceRecord,
    chunks: &[(String, SourceLocator)],
) -> Vec<EvidenceSpan> {
    chunks
        .iter()
        .map(|(text, locator)| EvidenceSpan {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            source_id: source.id.clone(),
            locator: locator.clone(),
            text: text.clone(),
            summary: None,
            extracted_at: Utc::now(),
        })
        .collect()
}

/// Extract evidence from all sources (deterministic only).
/// First chunks deterministically, then creates evidence spans.
pub fn extract_evidence(
    run_id: &str,
    sources: &[SourceRecord],
    source_contents: &[(String, String)], // (source_id, content) pairs
    budget: &ResearchBudget,
) -> Vec<EvidenceSpan> {
    let mut all_evidence = Vec::new();
    for (source_id, content) in source_contents {
        if let Some(source) = sources.iter().find(|s| &s.id == source_id) {
            let chunks = chunk_source_content(source, content, budget.max_chunks_per_source);
            let evidence = deterministic_evidence(run_id, source, &chunks);
            all_evidence.extend(evidence);
        }
    }
    // Trim to budget
    all_evidence.truncate(budget.max_evidence_spans);
    all_evidence
}

/// Model-backed evidence extraction for a single source chunk.
///
/// Calls the LLM with the EVIDENCE_EXTRACTION_PROMPT template to extract
/// structured evidence from a chunk of source content. Returns parsed
/// evidence spans, or None if the model returns empty/invalid results.
async fn model_evidence_for_chunk(
    provider: &dyn Provider,
    model: &str,
    question: &str,
    source: &SourceRecord,
    chunk_text: &str,
    locator: &SourceLocator,
    max_spans: usize,
) -> Option<Vec<ModelEvidenceItem>> {
    let locator_str = match locator {
        SourceLocator::FileRange {
            path,
            start_line,
            end_line,
        } => format!("{}:{}-{}", path.display(), start_line, end_line),
        SourceLocator::Url { url, heading } => {
            format!("{} ({})", url, heading.as_deref().unwrap_or("no heading"))
        }
        SourceLocator::TextSpan { label } => label.clone(),
    };

    let prompt = templates::EVIDENCE_EXTRACTION_PROMPT
        .replace("{question}", question)
        .replace("{source_id}", &source.id)
        .replace("{locator}", &locator_str);

    // Truncate chunk if very long to fit in context
    let truncated_chunk = if chunk_text.len() > 8000 {
        format!("{}...(truncated)", &chunk_text[..8000])
    } else {
        chunk_text.to_string()
    };

    let user_msg = format!("{}\n\n--- Source Content ---\n{}", prompt, truncated_chunk);

    let json_val = llm::call_llm_json(provider, model, None, &user_msg, Some(2048))
        .await
        .ok()?;

    let items: Vec<ModelEvidenceItem> = serde_json::from_value(json_val).ok()?;

    Some(items.into_iter().take(max_spans).collect())
}

/// Parsed evidence item from model response.
#[derive(serde::Deserialize)]
struct ModelEvidenceItem {
    text: String,
    summary: Option<String>,
    #[allow(dead_code)]
    relevance: Option<String>,
    #[allow(dead_code)]
    caveats: Option<Vec<String>>,
}

/// Extract evidence from all sources with optional model refinement.
///
/// When a provider is available, calls the LLM for each source chunk to extract
/// structured evidence with summaries. Falls back to deterministic extraction
/// on any LLM error or when provider is None.
pub async fn extract_evidence_with_model(
    run_id: &str,
    sources: &[SourceRecord],
    source_contents: &[(String, String)],
    budget: &ResearchBudget,
    provider: Option<&dyn Provider>,
    model: Option<&str>,
    question: &str,
) -> Vec<EvidenceSpan> {
    let Some(provider) = provider else {
        return extract_evidence(run_id, sources, source_contents, budget);
    };
    let Some(model) = model else {
        return extract_evidence(run_id, sources, source_contents, budget);
    };

    let mut all_evidence = Vec::new();
    let spans_per_chunk = 5;

    for (source_id, content) in source_contents {
        let Some(source) = sources.iter().find(|s| &s.id == source_id) else {
            continue;
        };

        let chunks = chunk_source_content(source, content, budget.max_chunks_per_source);

        for (chunk_text, locator) in &chunks {
            // Try model-backed extraction
            if let Some(items) = model_evidence_for_chunk(
                provider,
                model,
                question,
                source,
                chunk_text,
                locator,
                spans_per_chunk,
            )
            .await
            {
                for item in items {
                    all_evidence.push(EvidenceSpan {
                        id: uuid::Uuid::new_v4().to_string(),
                        run_id: run_id.to_string(),
                        source_id: source.id.clone(),
                        locator: locator.clone(),
                        text: item.text,
                        summary: item.summary,
                        extracted_at: Utc::now(),
                    });
                }
            } else {
                // Fallback: deterministic evidence for this chunk
                let ev = deterministic_evidence(
                    run_id,
                    source,
                    &[(chunk_text.clone(), locator.clone())],
                );
                all_evidence.extend(ev);
            }
        }
    }

    all_evidence.truncate(budget.max_evidence_spans);
    all_evidence
}

impl SourceLocator {
    /// Extract the file path if this locator is a FileRange variant.
    pub fn as_file_path(&self) -> Option<&std::path::Path> {
        match self {
            SourceLocator::FileRange { path, .. } => Some(path.as_path()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_source(source_type: SourceType, uri: &str) -> SourceRecord {
        SourceRecord {
            id: "src-1".to_string(),
            run_id: "run-1".to_string(),
            uri: uri.to_string(),
            title: None,
            source_type,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::TextSpan {
                label: "root".to_string(),
            },
            notes: vec![],
        }
    }

    #[test]
    fn chunk_local_file_basic() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        let content: String = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_local_file(&source, &content, 10);
        assert_eq!(chunks.len(), 1);
        match &chunks[0].1 {
            SourceLocator::FileRange {
                start_line,
                end_line,
                ..
            } => {
                assert_eq!(*start_line, 1);
                assert_eq!(*end_line, 50);
            }
            _ => panic!("Expected FileRange"),
        }
    }

    #[test]
    fn chunk_local_file_with_overlap() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        let content: String = (1..=150)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_local_file(&source, &content, 10);
        assert_eq!(chunks.len(), 2);
        // Second chunk should start at line 91 (100 - 10 overlap + 1)
        match &chunks[1].1 {
            SourceLocator::FileRange { start_line, .. } => {
                assert_eq!(*start_line, 91);
            }
            _ => panic!("Expected FileRange"),
        }
    }

    #[test]
    fn chunk_local_file_skips_empty() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        // 100 non-blank lines followed by 200 blank lines
        // Chunk 1: lines 1-100 (non-blank, kept)
        // Chunk 2: lines 91-200 (lines 91-100 non-blank, 101-200 blank - not entirely empty)
        // Chunk 3: lines 181-200 (entirely blank - skipped)
        let lines: Vec<String> = (1..=300)
            .map(|i| {
                if i <= 100 {
                    format!("line {i}")
                } else {
                    "   ".to_string()
                }
            })
            .collect();
        let content = lines.join("\n");
        let chunks = chunk_local_file(&source, &content, 10);
        // Chunk 2 has lines 91-100 non-blank, so it's kept. Chunk 3 is all blank, skipped.
        // We should get 2 chunks, not 3.
        assert!(chunks.len() <= 2);
    }

    #[test]
    fn chunk_url_text_on_headings() {
        let source = make_source(SourceType::Url, "https://example.com");
        let content = "# Title\nSome intro.\n\n## Section 1\nBody of section 1.\n\n## Section 2\nBody of section 2.";
        let chunks = chunk_url_text(&source, content, 10);
        // Should split on headings: before "# Title", "## Section 1", "## Section 2"
        assert!(chunks.len() >= 2);
        for (_, locator) in &chunks {
            assert!(matches!(locator, SourceLocator::TextSpan { .. }));
        }
    }

    #[test]
    fn chunk_url_text_char_fallback() {
        let source = make_source(SourceType::Url, "https://example.com");
        let content = "a".repeat(5000);
        let chunks = chunk_url_text(&source, &content, 10);
        assert!(chunks.len() >= 2);
        // All text should be preserved
        let total: usize = chunks.iter().map(|(t, _)| t.len()).sum();
        assert_eq!(total, 5000);
    }

    #[test]
    fn deterministic_evidence_creates_spans() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        let chunks = vec![
            (
                "chunk one".to_string(),
                SourceLocator::TextSpan {
                    label: "a".to_string(),
                },
            ),
            (
                "chunk two".to_string(),
                SourceLocator::TextSpan {
                    label: "b".to_string(),
                },
            ),
        ];
        let spans = deterministic_evidence("run-1", &source, &chunks);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].run_id, "run-1");
        assert_eq!(spans[0].source_id, "src-1");
        assert_eq!(spans[0].text, "chunk one");
        assert!(spans[0].summary.is_none());
    }

    #[test]
    fn extract_evidence_trims_to_budget() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        let content: String = (1..=500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let budget = ResearchBudget {
            max_sources: 10,
            max_chunks_per_source: 100,
            max_evidence_spans: 3,
            max_model_calls: 0,
            max_output_tokens: None,
            allow_network: false,
        };
        let evidence = extract_evidence(
            "run-1",
            &[source],
            &[("src-1".to_string(), content)],
            &budget,
        );
        assert_eq!(evidence.len(), 3);
    }

    #[test]
    fn chunk_empty_content() {
        let source = make_source(SourceType::LocalFile, "foo.rs");
        let chunks = chunk_local_file(&source, "", 10);
        assert!(chunks.is_empty());
    }
}
