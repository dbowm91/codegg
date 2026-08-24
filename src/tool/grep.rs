use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use ignore::WalkBuilder;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::error::ToolError;
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::util::{canonicalize_path, validate_path};
use crate::tool::{Tool, ToolCategory};

const MAX_GLOBAL_RESULTS: usize = 10_000;
const MAX_PER_FILE_RESULTS: usize = 1_000;
const MAX_PATTERN_SIZE: usize = 4096;
const MAX_PATTERN_GROUPS: usize = 32;
const MAX_WALK_ENTRIES: usize = 100_000;
const MAX_CONCURRENT_GREP: usize = 100;
const MAX_CONTEXT_LINES: usize = 1_000;
const MAX_CONTEXT_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RENDERED_BYTES: usize = 4 * 1024 * 1024;

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

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            cache_policy: ToolCachePolicy {
                enabled: true,
                ttl_secs: 60,
                max_entries: 50,
            },
            output_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "matches": {"type": "array", "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "line": {"type": "integer"},
                            "content": {"type": "string"}
                        }
                    }},
                    "total_matches": {"type": "integer"},
                    "files_searched": {"type": "integer"},
                    "truncated": {"type": "boolean"}
                },
                "required": ["matches"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
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

        let test_re = regex::Regex::new(&pattern)
            .map_err(|e| ToolError::Execution(format!("invalid regex: {e}")))?;
        let group_count = test_re.capture_names().flatten().count();
        if group_count > MAX_PATTERN_GROUPS {
            return Err(ToolError::Execution(format!(
                "pattern has too many capture groups ({}), maximum is {}",
                group_count, MAX_PATTERN_GROUPS
            )));
        }

        let search_path_str = input["path"].as_str().unwrap_or(".");
        let search_path = Path::new(search_path_str);
        let context = input["context"]
            .as_u64()
            .unwrap_or(0)
            .min(MAX_CONTEXT_LINES as u64) as usize;

        let allowed_root = self.allowed_root.clone();
        let unrestricted = self.unrestricted;

        let canonical_search = if unrestricted {
            canonicalize_path(search_path)?
        } else {
            validate_path(search_path, &allowed_root)?
        };

        let walk = WalkBuilder::new(search_path)
            .hidden(false)
            .git_ignore(true)
            .follow_links(false)
            .build();

        let canonical_search = canonical_search.clone();

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

        let (mut files, metrics, control) =
            run_search_batches(entries, pattern.clone(), context, MAX_CONCURRENT_GREP).await?;
        files.sort_by(|left, right| left.path.cmp(&right.path));

        let mut results = Vec::new();
        let mut rendered_bytes = result.len();
        let mut output_truncated = false;
        for file in files {
            for line in file.lines {
                if results.len() >= MAX_GLOBAL_RESULTS {
                    output_truncated = true;
                    break;
                }
                let rendered = format!("{}: {}", file.path.display(), line);
                if rendered_bytes.saturating_add(rendered.len() + 1) > MAX_RENDERED_BYTES {
                    output_truncated = true;
                    break;
                }
                rendered_bytes = rendered_bytes.saturating_add(rendered.len() + 1);
                results.push(rendered);
            }
            if output_truncated {
                break;
            }
        }

        if control.result_limit_reached() || control.output_limit_reached() {
            output_truncated = true;
        }

        let files_skipped = metrics.files_skipped.load(Ordering::Relaxed);
        tracing::debug!(
            worker_count = metrics.worker_count,
            blocking_tasks = metrics.blocking_tasks_created.load(Ordering::Relaxed),
            max_active_workers = metrics.max_active_workers.load(Ordering::Relaxed),
            context_reads = metrics.context_reads.load(Ordering::Relaxed),
            files_skipped,
            "grep search completed"
        );

        if results.is_empty() {
            result.push_str("No matches found.");
        } else {
            result.push_str(&results.join("\n"));
        }
        if output_truncated {
            result.push_str("\n\n[results truncated by grep limits]");
        }
        if files_skipped > 0 {
            result.push_str(&format!(
                "\n\n[{files_skipped} file(s) skipped: unreadable]"
            ));
        }
        Ok(result)
    }
}

struct GrepSink {
    matches: Vec<(usize, String)>,
    hit_limit: bool,
    control: Arc<SearchControl>,
}

impl GrepSink {
    fn new(control: Arc<SearchControl>) -> Self {
        Self {
            matches: Vec::new(),
            hit_limit: false,
            control,
        }
    }
}

impl Sink for GrepSink {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.hit_limit || self.control.is_stopped() || !self.control.claim_match() {
            self.hit_limit = true;
            return Ok(false);
        }
        let line = String::from_utf8_lossy(mat.bytes()).to_string();
        let line_num = mat.line_number().unwrap_or(0) as usize;
        self.matches.push((line_num, line));
        if self.matches.len() >= MAX_PER_FILE_RESULTS {
            self.hit_limit = true;
        }
        Ok(true)
    }
}

#[derive(Default)]
struct SearchControl {
    cancelled: AtomicBool,
    result_limit: AtomicBool,
    output_limit: AtomicBool,
    claimed_matches: AtomicUsize,
    rendered_bytes: AtomicUsize,
}

impl SearchControl {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self.result_limit.load(Ordering::Acquire)
            || self.output_limit.load(Ordering::Acquire)
    }

    fn claim_match(&self) -> bool {
        let mut current = self.claimed_matches.load(Ordering::Relaxed);
        loop {
            if current >= MAX_GLOBAL_RESULTS || self.is_stopped() {
                self.result_limit.store(true, Ordering::Release);
                return false;
            }
            match self.claimed_matches.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn reserve_rendered_bytes(&self, bytes: usize) -> bool {
        let mut current = self.rendered_bytes.load(Ordering::Relaxed);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                self.output_limit.store(true, Ordering::Release);
                return false;
            };
            if next > MAX_RENDERED_BYTES || self.is_stopped() {
                self.output_limit.store(true, Ordering::Release);
                return false;
            }
            match self.rendered_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn result_limit_reached(&self) -> bool {
        self.result_limit.load(Ordering::Acquire)
    }

    fn output_limit_reached(&self) -> bool {
        self.output_limit.load(Ordering::Acquire)
    }
}

struct SearchMetrics {
    worker_count: usize,
    blocking_tasks_created: AtomicUsize,
    active_workers: AtomicUsize,
    max_active_workers: AtomicUsize,
    context_reads: AtomicUsize,
    files_skipped: AtomicUsize,
}

impl SearchMetrics {
    fn new(worker_count: usize) -> Self {
        Self {
            worker_count,
            blocking_tasks_created: AtomicUsize::new(0),
            active_workers: AtomicUsize::new(0),
            max_active_workers: AtomicUsize::new(0),
            context_reads: AtomicUsize::new(0),
            files_skipped: AtomicUsize::new(0),
        }
    }

    fn worker_started(&self) {
        let active = self.active_workers.fetch_add(1, Ordering::AcqRel) + 1;
        let mut maximum = self.max_active_workers.load(Ordering::Relaxed);
        while active > maximum {
            match self.max_active_workers.compare_exchange_weak(
                maximum,
                active,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => maximum = observed,
            }
        }
    }

    fn worker_finished(&self) {
        self.active_workers.fetch_sub(1, Ordering::AcqRel);
    }
}

struct FileSearchResult {
    path: PathBuf,
    lines: Vec<String>,
}

fn partition_paths(paths: &[PathBuf], worker_count: usize) -> Vec<Vec<PathBuf>> {
    let workers = worker_count.min(paths.len()).max(1);
    let batch_size = paths.len().div_ceil(workers);
    paths
        .chunks(batch_size)
        .map(|batch| batch.to_vec())
        .collect()
}

async fn run_search_batches(
    entries: Vec<PathBuf>,
    pattern: String,
    context: usize,
    worker_limit: usize,
) -> Result<(Vec<FileSearchResult>, SearchMetrics, Arc<SearchControl>), ToolError> {
    let worker_count = worker_limit.min(entries.len()).max(1);
    let batches = partition_paths(&entries, worker_count);
    let control = Arc::new(SearchControl::default());
    let metrics = Arc::new(SearchMetrics::new(worker_count));
    let semaphore = Arc::new(Semaphore::new(worker_count));

    // There is one blocking task per deterministic batch, never one task per file.
    let worker_futures = batches.into_iter().map(|batch| {
        let pattern = pattern.clone();
        let control = Arc::clone(&control);
        let metrics = Arc::clone(&metrics);
        let semaphore = Arc::clone(&semaphore);
        async move {
            let permit = semaphore
                .acquire_owned()
                .await
                .map_err(|error| ToolError::Execution(format!("Semaphore error: {error}")))?;
            metrics
                .blocking_tasks_created
                .fetch_add(1, Ordering::Relaxed);
            let join = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                metrics.worker_started();
                let result =
                    search_batch(&batch, &pattern, context, Arc::clone(&control), &metrics);
                metrics.worker_finished();
                result
            });
            join.await
                .map_err(|error| ToolError::Execution(format!("Grep task failed: {error}")))?
        }
    });

    let joined = tokio::time::timeout(
        Duration::from_secs(30),
        futures_util::future::join_all(worker_futures),
    )
    .await;
    let worker_results = match joined {
        Ok(results) => results,
        Err(_) => {
            control.cancel();
            return Err(ToolError::Execution(
                "grep timeout after 30 seconds".to_string(),
            ));
        }
    };

    let mut files = Vec::new();
    for result in worker_results {
        match result {
            Ok(batch_results) => files.extend(batch_results),
            Err(error) => tracing::warn!("Grep worker failed: {error}"),
        }
    }

    let metrics = Arc::try_unwrap(metrics).unwrap_or_else(|_| {
        unreachable!("all grep worker references are joined before metrics are returned")
    });
    Ok((files, metrics, control))
}

fn search_batch(
    paths: &[PathBuf],
    pattern: &str,
    context: usize,
    control: Arc<SearchControl>,
    metrics: &SearchMetrics,
) -> Result<Vec<FileSearchResult>, ToolError> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|error| ToolError::Execution(format!("invalid regex: {error}")))?;
    let mut results = Vec::new();

    for path in paths {
        if control.is_stopped() {
            break;
        }
        if let Some(result) = search_file(path, &matcher, context, Arc::clone(&control), metrics) {
            results.push(result);
        }
    }
    Ok(results)
}

fn search_file(
    path: &Path,
    matcher: &RegexMatcher,
    context: usize,
    control: Arc<SearchControl>,
    metrics: &SearchMetrics,
) -> Option<FileSearchResult> {
    let mut searcher = Searcher::new();
    let mut sink = GrepSink::new(Arc::clone(&control));
    if let Err(error) = searcher.search_path(matcher, path, &mut sink) {
        metrics.files_skipped.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(path = %path.display(), %error, "grep skipped unreadable file");
    }
    if sink.matches.is_empty() {
        return None;
    }

    let lines = render_file_matches(path, &sink.matches, context, control, metrics);
    Some(FileSearchResult {
        path: path.to_path_buf(),
        lines,
    })
}

struct ContextSnapshot {
    lines: Vec<String>,
}

impl ContextSnapshot {
    fn from_path(path: &Path, metrics: &SearchMetrics) -> Option<Self> {
        metrics.context_reads.fetch_add(1, Ordering::Relaxed);
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.len() > MAX_CONTEXT_FILE_BYTES as u64 {
            return None;
        }
        let content = std::fs::read(path).ok()?;
        if content.len() > MAX_CONTEXT_FILE_BYTES {
            return None;
        }
        let content = String::from_utf8(content).ok()?;
        Some(Self {
            lines: content.lines().map(ToOwned::to_owned).collect(),
        })
    }

    fn line(&self, line_number: usize) -> Option<&str> {
        line_number
            .checked_sub(1)
            .and_then(|index| self.lines.get(index).map(String::as_str))
    }

    fn line_count(&self) -> usize {
        self.lines.len()
    }
}

fn render_file_matches(
    path: &Path,
    matches: &[(usize, String)],
    context: usize,
    control: Arc<SearchControl>,
    metrics: &SearchMetrics,
) -> Vec<String> {
    let snapshot = (context > 0).then(|| ContextSnapshot::from_path(path, metrics));
    let mut rendered = Vec::new();
    let mut emitted_context = BTreeSet::new();

    for (line_number, line) in matches {
        if control.is_stopped() {
            break;
        }
        if let Some(Some(snapshot)) = snapshot.as_ref() {
            let start = line_number.saturating_sub(context).max(1);
            let end = line_number
                .saturating_add(context)
                .min(snapshot.line_count());
            for context_line in start..*line_number {
                if emitted_context.insert(context_line) {
                    if let Some(text) = snapshot.line(context_line) {
                        if !push_rendered_line(
                            &mut rendered,
                            path,
                            context_line,
                            text,
                            true,
                            control.as_ref(),
                        ) {
                            return rendered;
                        }
                    }
                }
            }
            if !push_rendered_line(
                &mut rendered,
                path,
                *line_number,
                line,
                false,
                control.as_ref(),
            ) {
                return rendered;
            }
            for context_line in line_number.saturating_add(1)..=end {
                if emitted_context.insert(context_line) {
                    if let Some(text) = snapshot.line(context_line) {
                        if !push_rendered_line(
                            &mut rendered,
                            path,
                            context_line,
                            text,
                            true,
                            control.as_ref(),
                        ) {
                            return rendered;
                        }
                    }
                }
            }
        } else if !push_rendered_line(
            &mut rendered,
            path,
            *line_number,
            line,
            false,
            control.as_ref(),
        ) {
            return rendered;
        }
    }
    rendered
}

fn push_rendered_line(
    rendered: &mut Vec<String>,
    path: &Path,
    line_number: usize,
    text: &str,
    is_context: bool,
    control: &SearchControl,
) -> bool {
    let line = if is_context {
        format!("{}:{}:- {}", path.display(), line_number, text)
    } else {
        format!("{}:{}:{}", path.display(), line_number, text)
    };
    if !control.reserve_rendered_bytes(line.len().saturating_add(1)) {
        return false;
    }
    rendered.push(line);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn paths(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|index| PathBuf::from(format!("file-{index:04}.txt")))
            .collect()
    }

    #[test]
    fn partitioning_never_creates_more_batches_than_workers() {
        let entries = paths(25);
        let batches = partition_paths(&entries, 4);

        assert_eq!(batches.len(), 4);
        assert!(batches.iter().all(|batch| !batch.is_empty()));
        assert_eq!(batches.concat(), entries);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn blocking_task_count_and_active_workers_are_bounded() {
        let directory = tempdir().unwrap();
        let entries = (0..120)
            .map(|index| {
                let path = directory.path().join(format!("file-{index:04}.txt"));
                fs::write(&path, "no match\n").unwrap();
                path
            })
            .collect::<Vec<_>>();

        let (_, metrics, _) = run_search_batches(entries, "needle".into(), 0, 4)
            .await
            .unwrap();

        assert_eq!(metrics.worker_count, 4);
        assert_eq!(metrics.blocking_tasks_created.load(Ordering::Acquire), 4);
        assert!(metrics.max_active_workers.load(Ordering::Acquire) <= 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn one_worker_limit_still_scans_all_files_serially() {
        let directory = tempdir().unwrap();
        let entries = (0..5)
            .map(|index| {
                let path = directory.path().join(format!("file-{index}.txt"));
                fs::write(&path, format!("needle {index}\n")).unwrap();
                path
            })
            .collect::<Vec<_>>();

        let (files, metrics, _) = run_search_batches(entries, "needle".into(), 0, 1)
            .await
            .unwrap();

        assert_eq!(files.len(), 5);
        assert_eq!(metrics.worker_count, 1);
        assert_eq!(metrics.blocking_tasks_created.load(Ordering::Acquire), 1);
        assert_eq!(metrics.max_active_workers.load(Ordering::Acquire), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn context_is_read_once_for_multiple_matches_in_one_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("matches.txt");
        fs::write(&path, "one\nneedle two\nthree\nneedle four\nfive\n").unwrap();

        let (files, metrics, _) = run_search_batches(vec![path.clone()], "needle".into(), 1, 1)
            .await
            .unwrap();

        assert_eq!(metrics.context_reads.load(Ordering::Acquire), 1);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].lines.len(), 5);
        assert!(files[0].lines[0].ends_with(":1:- one"));
        assert!(files[0].lines[1].contains(":2:needle two"));
        assert!(files[0].lines[2].ends_with(":3:- three"));
        assert!(files[0].lines[3].contains(":4:needle four"));
        assert!(files[0].lines[4].ends_with(":5:- five"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_merges_parallel_results_by_path_order() {
        let directory = tempdir().unwrap();
        let first = directory.path().join("a.txt");
        let second = directory.path().join("b.txt");
        fs::write(&first, "needle a\n").unwrap();
        fs::write(&second, "needle b\n").unwrap();
        let tool = GrepTool::new().with_allowed_root(directory.path().to_path_buf());

        let result = tool
            .execute(json!({
                "pattern": "needle",
                "path": directory.path().to_string_lossy(),
            }))
            .await
            .unwrap();

        assert!(result.find("a.txt").unwrap() < result.find("b.txt").unwrap());
    }

    #[test]
    fn cancellation_stops_a_batch_before_file_processing() {
        let control = Arc::new(SearchControl::default());
        control.cancel();
        let metrics = SearchMetrics::new(1);
        let directory = tempdir().unwrap();
        let path = directory.path().join("cancelled.txt");
        fs::write(&path, "needle\n").unwrap();

        let results = search_batch(&[path], "needle", 0, Arc::clone(&control), &metrics).unwrap();

        assert!(results.is_empty());
    }
}
