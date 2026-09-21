//! Tool catalog for search and discovery.
//!
//! This module provides the ToolCatalog for registering and searching tools.
//! It supports deferred loading of tools that should only be loaded on-demand,
//! and BM25 ranking for improved search relevance.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use super::Tool;

/// Search mode for tool catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Simple case-insensitive substring matching (default).
    #[default]
    Keyword,
    /// BM25 ranking based on term frequency and inverse document frequency.
    BM25,
}

impl SearchMode {
    /// Parse from a config string.
    pub fn from_config(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bm25" => SearchMode::BM25,
            _ => SearchMode::Keyword,
        }
    }
}

/// Tokenize text into lowercase alphanumeric terms.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Compute IDF (Inverse Document Frequency) for a set of tool documents.
fn compute_idf(tools: &[&ToolMetadata]) -> HashMap<String, f64> {
    let n = tools.len() as f64;
    if n == 0.0 {
        return HashMap::new();
    }

    let mut doc_freq: HashMap<String, usize> = HashMap::new();

    // Combine name + description as the "document" for each tool
    for tool in tools {
        let doc = format!("{} {}", tool.name, tool.description);
        let terms: HashSet<String> = tokenize(&doc).into_iter().collect();
        for term in terms {
            *doc_freq.entry(term).or_insert(0) += 1;
        }
    }

    doc_freq
        .into_iter()
        .map(|(term, df)| {
            // Standard BM25 IDF formula: ln((N - df + 0.5) / (df + 0.5) + 1)
            let idf = ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
            (term, idf)
        })
        .collect()
}

/// Compute average document length across all tools.
fn compute_avg_doc_length(tools: &[&ToolMetadata]) -> f64 {
    if tools.is_empty() {
        return 0.0;
    }
    let total_len: usize = tools
        .iter()
        .map(|t| tokenize(&format!("{} {}", t.name, t.description)).len())
        .sum();
    total_len as f64 / tools.len() as f64
}

/// BM25 scoring function for a query against a document.
fn bm25_score(query: &str, document: &str, avg_dl: f64, idf: &HashMap<String, f64>) -> f64 {
    let k1 = 1.5;
    let b = 0.75;

    let query_terms = tokenize(query);
    let doc_terms = tokenize(document);
    let doc_len = doc_terms.len() as f64;

    // Count term frequencies in document
    let mut tf: HashMap<String, usize> = HashMap::new();
    for term in &doc_terms {
        *tf.entry(term.clone()).or_insert(0) += 1;
    }

    let mut score = 0.0;
    for term in &query_terms {
        if let Some(&term_tf) = tf.get(term) {
            let idf_val = idf.get(term).copied().unwrap_or(0.0);
            let numerator = term_tf as f64 * (k1 + 1.0);
            let denominator = term_tf as f64 + k1 * (1.0 - b + b * doc_len / avg_dl);
            score += idf_val * numerator / denominator;
        }
    }
    score
}

/// Metadata about a tool for catalog/registry purposes.
///
/// `category` and `disclosure` are denormalized at registration so
/// `tool_search` can return effect/risk-relevant selection metadata
/// without re-resolving tool instances. They contain no secrets,
/// endpoints, credentials, or reasoning: only the static tool name,
/// description, JSON schema, category label, and disclosure state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,
    /// Static safety category label (`ReadOnly`, `SafeMutating`,
    /// `Mutating`, `ShellExec`).
    #[serde(default = "default_category_label")]
    pub category: String,
    /// Canonical disclosure state (`core`, `deferred`,
    /// `profile_specific`, `hidden`).
    #[serde(default = "default_disclosure_label")]
    pub disclosure: String,
}

fn default_category_label() -> String {
    "Mutating".to_string()
}

fn default_disclosure_label() -> String {
    "core".to_string()
}

impl ToolMetadata {
    pub fn from_tool(tool: &dyn Tool) -> Self {
        let category = match tool.category() {
            super::ToolCategory::ReadOnly => "ReadOnly",
            super::ToolCategory::SafeMutating => "SafeMutating",
            super::ToolCategory::Mutating => "Mutating",
            super::ToolCategory::ShellExec => "ShellExec",
        }
        .to_string();
        let name = tool.name().to_string();
        let disclosure = super::disclosure::disclosure_for(&name)
            .as_str()
            .to_string();
        Self {
            name,
            description: tool.description().to_string(),
            parameters: tool.parameters(),
            defer_load: tool.defer_loading(),
            category,
            disclosure,
        }
    }
}

/// Catalog of available tools with search capabilities.
///
/// The catalog maintains a mapping of tool names to their metadata,
/// and tracks which tools should be loaded on-demand (deferred).
/// Supports keyword (substring) or BM25 ranking search modes.
pub struct ToolCatalog {
    inner: Arc<RwLock<ToolCatalogState>>,
}

#[derive(Clone)]
struct ToolCatalogState {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
    search_mode: SearchMode,
    // Pre-computed BM25 statistics
    avg_doc_length: f64,
    doc_count: usize,
    idf_cache: HashMap<String, f64>,
}

impl Clone for ToolCatalog {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ToolCatalog {
    /// Rank a bounded set of descriptors using the same keyword/BM25
    /// implementation as the live catalog. This is used by the offline
    /// advisor benchmark and intentionally does not register or execute tools.
    pub fn rank_descriptors(
        query: &str,
        metadata: &[ToolMetadata],
        mode: SearchMode,
    ) -> Vec<ToolMetadata> {
        if mode == SearchMode::Keyword {
            let query_lower = query.to_lowercase();
            let mut results: Vec<_> = metadata
                .iter()
                .filter(|metadata| {
                    metadata.name.to_lowercase().contains(&query_lower)
                        || metadata.description.to_lowercase().contains(&query_lower)
                })
                .cloned()
                .collect();
            results.sort_by(|left, right| left.name.cmp(&right.name));
            return results;
        }
        if query.trim().is_empty() || metadata.is_empty() {
            return Vec::new();
        }
        let refs: Vec<&ToolMetadata> = metadata.iter().collect();
        let idf = compute_idf(&refs);
        let avg_dl = compute_avg_doc_length(&refs);
        let mut scored: Vec<(ToolMetadata, f64)> = metadata
            .iter()
            .map(|tool| {
                let score = bm25_score(
                    query,
                    &format!("{} {}", tool.name, tool.description),
                    avg_dl,
                    &idf,
                );
                (tool.clone(), score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.name.cmp(&right.0.name))
        });
        scored.into_iter().map(|(tool, _)| tool).collect()
    }

    /// Create a new empty tool catalog with default keyword search.
    pub fn new() -> Self {
        Self::with_search_mode(SearchMode::default())
    }

    /// Create a new empty tool catalog with the specified search mode.
    pub fn with_search_mode(mode: SearchMode) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolCatalogState {
                tools: HashMap::new(),
                deferred_load: Vec::new(),
                search_mode: mode,
                avg_doc_length: 0.0,
                doc_count: 0,
                idf_cache: HashMap::new(),
            })),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, ToolCatalogState> {
        self.inner.read().unwrap_or_else(|error| error.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, ToolCatalogState> {
        self.inner
            .write()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Get the current search mode.
    pub fn search_mode(&self) -> SearchMode {
        self.read().search_mode
    }

    /// Set the search mode and recompute BM25 caches if switching to BM25.
    pub fn set_search_mode(&mut self, mode: SearchMode) {
        let mut state = self.write();
        state.search_mode = mode;
        if mode == SearchMode::BM25 {
            Self::recompute_bm25_caches(&mut state);
        }
    }

    /// Recompute BM25 IDF cache and average document length.
    fn recompute_bm25_caches(state: &mut ToolCatalogState) {
        let tools: Vec<&ToolMetadata> = state.tools.values().collect();
        state.doc_count = tools.len();
        state.avg_doc_length = compute_avg_doc_length(&tools);
        state.idf_cache = compute_idf(&tools);
    }

    /// Register a tool in the catalog.
    pub fn register(&mut self, tool: &dyn Tool) {
        let metadata = ToolMetadata::from_tool(tool);
        let name = metadata.name.clone();

        let mut state = self.write();
        state.deferred_load.retain(|deferred| deferred != &name);
        if metadata.defer_load {
            state.deferred_load.push(name.clone());
        }

        state.tools.insert(name, metadata);

        // Recompute BM25 caches if in BM25 mode
        if state.search_mode == SearchMode::BM25 {
            Self::recompute_bm25_caches(&mut state);
        }
    }

    /// Search tools by name or description.
    ///
    /// Uses the configured search mode (keyword or BM25).
    pub fn search(&self, query: &str) -> Vec<ToolMetadata> {
        match self.search_mode() {
            SearchMode::Keyword => self.keyword_search(query),
            SearchMode::BM25 => self.bm25_search(query),
        }
    }

    /// Simple case-insensitive substring search (original behavior).
    fn keyword_search(&self, query: &str) -> Vec<ToolMetadata> {
        let query_lower = query.to_lowercase();

        let mut results: Vec<_> = self
            .read()
            .tools
            .values()
            .filter(|metadata| {
                metadata.name.to_lowercase().contains(&query_lower)
                    || metadata.description.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();
        results.sort_by(|left, right| left.name.cmp(&right.name));
        results
    }

    /// BM25 ranked search.
    ///
    /// Returns tools ranked by BM25 score, filtering out zero-score results.
    fn bm25_search(&self, query: &str) -> Vec<ToolMetadata> {
        if query.trim().is_empty() {
            return Vec::new();
        }

        let state = self.read();
        let tools: Vec<&ToolMetadata> = state.tools.values().collect();
        let avg_dl = state.avg_doc_length;

        let mut scored: Vec<(ToolMetadata, f64)> = tools
            .iter()
            .map(|tool| {
                let doc = format!("{} {}", tool.name, tool.description);
                let score = bm25_score(query, &doc, avg_dl, &state.idf_cache);
                ((*tool).clone(), score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.name.cmp(&right.0.name))
        });
        scored.into_iter().map(|(tool, _)| tool).collect()
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<ToolMetadata> {
        self.read().tools.get(name).cloned()
    }

    /// List all tools marked for deferred loading.
    pub fn deferred_tools(&self) -> Vec<ToolMetadata> {
        let state = self.read();
        state
            .deferred_load
            .iter()
            .filter_map(|name| state.tools.get(name).cloned())
            .collect()
    }

    /// List all tools in the catalog.
    pub fn list(&self) -> Vec<ToolMetadata> {
        let mut tools: Vec<_> = self.read().tools.values().cloned().collect();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        tools
    }

    /// Check if a tool is marked for deferred loading.
    pub fn is_deferred(&self, name: &str) -> bool {
        self.read()
            .deferred_load
            .iter()
            .any(|deferred| deferred == name)
    }

    /// Register additional tool names as deferred (e.g., from config).
    pub fn register_deferred_names(&mut self, names: &[String]) {
        let mut state = self.write();
        for name in names {
            if !state.deferred_load.contains(name) {
                state.deferred_load.push(name.clone());
            }
        }
    }
}

impl Default for ToolCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use async_trait::async_trait;
    use serde_json::json;

    struct MockTool {
        name: String,
        description: String,
        defer: bool,
    }

    impl MockTool {
        fn new(name: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
                defer: false,
            }
        }

        fn deferred(name: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
                defer: true,
            }
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            &self.description
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        async fn execute(&self, _: serde_json::Value) -> Result<String, crate::error::ToolError> {
            Ok("ok".into())
        }
        fn defer_loading(&self) -> bool {
            self.defer
        }
    }

    // --- Keyword search tests (backward compatibility) ---

    #[test]
    fn keyword_search_by_name() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("bash", "Execute shell commands"));
        catalog.register(&MockTool::new("read", "Read file contents"));

        let results = catalog.search("bash");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "bash");
    }

    #[test]
    fn keyword_search_by_description() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("bash", "Execute shell commands"));
        catalog.register(&MockTool::new("read", "Read file contents"));

        let results = catalog.search("shell");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "bash");
    }

    #[test]
    fn keyword_search_case_insensitive() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("Bash", "Execute Shell Commands"));

        let results = catalog.search("bash");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn keyword_search_no_results() {
        let catalog = ToolCatalog::new();
        let results = catalog.search("nonexistent");
        assert!(results.is_empty());
    }

    // --- BM25 search tests ---

    #[test]
    fn bm25_search_returns_ranked_results() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new(
            "bash",
            "Execute bash shell commands on the system",
        ));
        catalog.register(&MockTool::new(
            "read",
            "Read and display file contents from disk",
        ));
        catalog.register(&MockTool::new(
            "write",
            "Write content to files on the filesystem",
        ));

        // "bash" should match the bash tool
        let results = catalog.search("bash");
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "bash");
    }

    #[test]
    fn bm25_search_multi_word_query() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new(
            "validate_json",
            "Validate JSON format and structure",
        ));
        catalog.register(&MockTool::new("read_file", "Read file contents from disk"));
        catalog.register(&MockTool::new("parse_config", "Parse configuration files"));

        // "json validate" should match validate_json
        let results = catalog.search("json validate");
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "validate_json");
    }

    #[test]
    fn bm25_search_empty_query_returns_empty() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new("bash", "Execute shell commands"));

        let results = catalog.search("");
        assert!(results.is_empty());
    }

    #[test]
    fn bm25_search_whitespace_only_returns_empty() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new("bash", "Execute shell commands"));

        let results = catalog.search("   ");
        assert!(results.is_empty());
    }

    #[test]
    fn bm25_search_single_word() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new(
            "bash",
            "Execute bash shell commands on the system",
        ));
        catalog.register(&MockTool::new(
            "read",
            "Read and display file contents from disk",
        ));

        let results = catalog.search("execute");
        assert!(!results.is_empty());
        assert!(results.iter().any(|m| m.name == "bash"));
    }

    #[test]
    fn bm25_no_results_for_unrelated_query() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new("bash", "Execute shell commands"));
        catalog.register(&MockTool::new("read", "Read file contents"));

        let results = catalog.search("xyznonexistent");
        assert!(results.is_empty());
    }

    // --- Deferred loading tests ---

    #[test]
    fn deferred_tools_tracked() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("bash", "Execute shell commands"));
        catalog.register(&MockTool::deferred("special_tool", "A special tool"));

        let deferred = catalog.deferred_tools();
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].name, "special_tool");
    }

    #[test]
    fn cloned_catalog_observes_late_registration_and_replacement() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("initial", "initial tool"));
        let shared = catalog.clone();

        catalog.register(&MockTool::deferred("late", "late discoverable tool"));
        assert_eq!(
            shared.get("late").expect("late tool is shared").name,
            "late"
        );
        assert!(shared.is_deferred("late"));

        catalog.register(&MockTool::new("late", "replacement tool"));
        let replacement = shared.get("late").expect("replacement is current");
        assert_eq!(replacement.description, "replacement tool");
        assert!(!shared.is_deferred("late"));
    }

    #[test]
    fn catalog_search_order_is_deterministic() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("zeta", "shared match"));
        catalog.register(&MockTool::new("alpha", "shared match"));
        let names: Vec<_> = catalog
            .search("shared")
            .into_iter()
            .map(|metadata| metadata.name)
            .collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    // --- Search mode tests ---

    #[test]
    fn search_mode_from_config() {
        assert_eq!(SearchMode::from_config("keyword"), SearchMode::Keyword);
        assert_eq!(SearchMode::from_config("bm25"), SearchMode::BM25);
        assert_eq!(SearchMode::from_config("BM25"), SearchMode::BM25);
        assert_eq!(SearchMode::from_config("embeddings"), SearchMode::Keyword);
        assert_eq!(SearchMode::from_config(""), SearchMode::Keyword);
    }

    #[test]
    fn set_search_mode_recomputes_caches() {
        let mut catalog = ToolCatalog::new();
        catalog.register(&MockTool::new("bash", "Execute shell commands"));
        catalog.register(&MockTool::new("read", "Read file contents"));

        // Switch to BM25 mode
        catalog.set_search_mode(SearchMode::BM25);
        assert_eq!(catalog.search_mode(), SearchMode::BM25);
        let state = catalog.read();
        assert_eq!(state.doc_count, 2);
        assert!(state.avg_doc_length > 0.0);
        assert!(!state.idf_cache.is_empty());
    }

    #[test]
    fn bm25_recomputes_on_register() {
        let mut catalog = ToolCatalog::with_search_mode(SearchMode::BM25);
        catalog.register(&MockTool::new("bash", "Execute shell commands"));

        let old_count = catalog.read().doc_count;
        catalog.register(&MockTool::new("read", "Read file contents"));

        // Cache should have been recomputed
        assert_eq!(catalog.read().doc_count, old_count + 1);
    }

    // --- BM25 scoring tests ---

    #[test]
    fn bm25_score_basic() {
        let mut idf = HashMap::new();
        idf.insert("bash".to_string(), 1.0);
        idf.insert("shell".to_string(), 1.0);

        let score = bm25_score("bash", "execute bash shell commands", 4.0, &idf);
        assert!(score > 0.0);
    }

    #[test]
    fn bm25_score_no_match() {
        let idf = HashMap::new();
        let score = bm25_score("xyz", "execute bash shell commands", 4.0, &idf);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn tokenize_basic() {
        let tokens = tokenize("Hello, World! This is a test.");
        assert_eq!(tokens, vec!["hello", "world", "this", "is", "a", "test"]);
    }

    #[test]
    fn tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_special_chars() {
        let tokens = tokenize("validate_json->parse");
        assert_eq!(tokens, vec!["validate", "json", "parse"]);
    }
}
