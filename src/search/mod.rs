//! Legacy built-in web search providers.
//!
//! This module retains the original in-tree [`SearchProvider`]
//! implementations that powered `websearch` before the eggsearch
//! migration. New provider work should happen in the
//! [eggsearch](https://github.com/anomalyco/eggsearch) project.
//! Codegg's native `websearch` and `webfetch` tools are now thin
//! wrappers around eggsearch when
//! `[search].backend = "eggsearch"` (the default).
//!
//! The in-tree implementation is kept as an explicit fallback path
//! for users who cannot install eggsearch; it is selected with
//! `[search].backend = "builtin"`. It also runs automatically when
//! `[search].fallback_to_builtin = true` and the eggsearch backend
//! fails.
//!
//! ## Provider dispatch
//!
//! [`SearchProviderRegistry`] tries providers in a deterministic order:
//!
//! 1. **Key-based providers** (Exa, Tavily, Brave, Kagi, SerpAPI) — best
//!    result quality; used when an API key is set in the environment.
//! 2. **DuckDuckGo HTML** — default fallback, no key required, scrapes
//!    `https://html.duckduckgo.com/html/`. Returns real Bing-derived
//!    results with reasonable relevance.
//! 3. **Mojeek HTML** — last-resort fallback, no key required, scrapes
//!    `https://www.mojeek.com/search`. Independent index, useful as
//!    corroboration when DDG returns thin results.
//!
//! Domain-specific providers (Wikipedia, OpenAlex, arXiv, PubMed,
//! Hacker News Algolia, Google News RSS, GitHub) are added on demand
//! when the query shape matches their domain; they are not part of the
//! default dispatch chain.

pub mod arxiv;
pub mod duckduckgo;
pub mod github;
pub mod google_news;
pub mod hn_algolia;
pub mod mojeek;
pub mod openalex;
pub mod providers;
pub mod pubmed;
pub mod registry;
pub mod routing;
pub mod types;
pub mod wikipedia;

pub use registry::SearchProviderRegistry;
pub use routing::{classify_query, QueryKind};
pub use types::{SearchError, SearchHit, SearchProvider, Specificity};

/// Convenience: run a search using the default registry. Used by the
/// `[search].backend = "builtin"` fallback path; the eggsearch backend
/// has its own dispatcher in `crate::search_backend`.
pub async fn search_default(
    query: &str,
    num_results: usize,
) -> Result<Vec<SearchHit>, SearchError> {
    let registry = SearchProviderRegistry::from_env();
    registry.search(query, num_results, None).await
}
