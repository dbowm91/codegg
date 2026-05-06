use async_trait::async_trait;
use grep::regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use ignore::WalkBuilder;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::error::ToolError;
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::Tool;

const MAX_GLOBAL_RESULTS: usize = 10_000;
const MAX_PER_FILE_RESULTS: usize = 1_000;
const MAX_PATTERN_SIZE: usize = 4096;
const MAX_PATTERN_GROUPS: usize = 32;
const MAX_WALK_ENTRIES: usize = 100_000;
const MAX_CONCURRENT_GREP: usize = 100;

pub struct GrepTool {
    allowed_root: PathBuf,
    unrestricted: bool,
}

impl GrepTool {
    pub fn new() -> Self {
        Self {
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            unrestricted: false,
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self.unrestricted = false;
        self
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regular expressions"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in"
                },
                "context": {
                    "type": "number",
                    "description": "Number of context lines before and after"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        if self.unrestricted {
            tracing::warn!("GrepTool executing with unrestricted=true - no path validation");
        }

        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'pattern' parameter".to_string()))?
            .to_string();

        if pattern.len() > MAX_PATTERN_SIZE {
            return Err(ToolError::Execution(format!(
                "pattern exceeds maximum size of {} bytes",
                MAX_PATTERN_SIZE
            )));
        }

        let group_count = pattern.matches('(').count();
        if group_count > MAX_PATTERN_GROUPS {
            return Err(ToolError::Execution(format!(
                "pattern has too many capture groups ({}), maximum is {}",
                group_count, MAX_PATTERN_GROUPS
            )));
        }

        let search_path_str = input["path"].as_str().unwrap_or(".");
        let search_path = Path::new(search_path_str);
        let context = input["context"].as_u64().unwrap_or(0) as usize;

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let canonical_search = if unrestricted {
            canonicalize_path(search_path)?
        } else {
            validate_path(search_path, &allowed_root)?
        };

        RegexMatcher::new(&pattern)
            .map_err(|e| ToolError::Execution(format!("invalid regex: {e}")))?;

        let walk = WalkBuilder::new(&search_path)
            .hidden(false)
            .git_ignore(true)
            .follow_links(false)
            .build();

        let canonical_search = canonical_search.clone();
        let unrestricted = self.unrestricted;

        let (entries, truncated) = tokio::task::spawn_blocking(move || {
            let mut entries = Vec::new();
            let mut truncated = false;
            for entry in walk {
                if entries.len() >= MAX_WALK_ENTRIES {
                    truncated = true;
                    break;
                }
                if let Ok(entry) = entry {
                    if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                        continue;
                    }
                    if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        let path = entry.into_path();
                        let Ok(canonical) = path.canonicalize() else {
                            continue;
                        };
                        if !unrestricted && !canonical.starts_with(&canonical_search) {
                            continue;
                        }
                        entries.push(canonical);
                    }
                }
            }
            (entries, truncated)
        })
        .await
        .map_err(|e| ToolError::Execution(format!("task join error: {}", e)))?;

        if entries.is_empty() {
            return Ok(format!("No matches for '{}'", pattern));
        }

        let mut result = format!("[searching {} files", entries.len());
        if truncated {
            result.push_str(&format!(" (truncated at {} entries)", MAX_WALK_ENTRIES));
        }
        result.push_str("]\n\n");

        let batch_size = (entries.len() / 4).max(10);
        let mut results = Vec::with_capacity(1000);
        let start = Instant::now();
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_GREP));

        for batch in entries.chunks(batch_size) {
            if start.elapsed() > Duration::from_secs(30) {
                return Err(ToolError::Execution(
                    "grep timeout after 30 seconds".to_string(),
                ));
            }

            if results.len() >= MAX_GLOBAL_RESULTS {
                break;
            }

            let futures: Vec<_> = batch
                .iter()
                .map(|path| {
                    let path = path.clone();
                    let pattern = pattern.clone();
                    let sem = Arc::clone(&sem);
                    async move {
                        let permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!("Semaphore acquire failed: {}", e);
                                return Err(ToolError::Execution(format!(
                                    "Semaphore error: {}",
                                    e
                                )));
                            }
                        };
                        drop(permit);
                        Ok(tokio::task::spawn_blocking(move || {
                            let matcher = RegexMatcher::new(&pattern)
                                .map_err(|e| ToolError::Execution(e.to_string()))?;
                            let mut searcher = Searcher::new();
                            let mut sink = GrepSink::new(&path, context);
                            let _ = searcher.search_path(&matcher, &path, &mut sink);
                            Ok::<_, ToolError>((path, sink.matches, sink.hit_limit))
                        }))
                    }
                })
                .collect();

            let handles: Vec<_> = futures::future::join_all(futures).await;

            for handle in handles {
                if results.len() >= MAX_GLOBAL_RESULTS {
                    break;
                }
                match handle {
                    Ok(join_handle) => {
                        if let Ok(Ok((path, matches, _hit_limit))) = join_handle.await {
                            for m in matches {
                                if results.len() >= MAX_GLOBAL_RESULTS {
                                    break;
                                }
                                results.push(format!("{}: {}", path.display(), m));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Grep task failed: {}", e);
                    }
                }
            }
        }

        if results.is_empty() {
            result.push_str("No matches found.");
            Ok(result)
        } else {
            result.push_str(&results.join("\n"));
            Ok(result)
        }
    }
}

struct GrepSink {
    path: std::path::PathBuf,
    matches: Vec<String>,
    hit_limit: bool,
}

impl GrepSink {
    fn new(path: &std::path::Path, _context: usize) -> Self {
        Self {
            path: path.to_path_buf(),
            matches: Vec::new(),
            hit_limit: false,
        }
    }
}

impl Sink for GrepSink {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.hit_limit {
            return Ok(false);
        }
        let line = String::from_utf8_lossy(mat.bytes()).to_string();
        let line_num = mat.line_number().unwrap_or(0);
        self.matches
            .push(format!("{}:{}:{}", self.path.display(), line_num, line));
        if self.matches.len() >= MAX_PER_FILE_RESULTS {
            self.hit_limit = true;
        }
        Ok(true)
    }
}
