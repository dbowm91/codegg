//! Provider dispatch + ordering.
//!
//! [`SearchProviderRegistry`] is the entry point the `websearch` tool
//! uses. It instantiates only the providers that have what they need
//! to operate (key-based providers are only included if their env var
//! is set) and dispatches queries in a deterministic order:
//!
//! 1. **General key-based providers** (Exa, Tavily, Brave, Kagi, SerpAPI)
//!    in env-var order. Whichever is configured and succeeds first
//!    wins; on failure, the next one is tried.
//! 2. **DuckDuckGo HTML** (no key, no configuration needed).
//! 3. **Mojeek HTML** (no key, no configuration needed) — last-resort
//!    fallback that uses an independent index.
//!
//! After the general chain, if results are thin (≤ 2 hits) and the
//! query shape matches a domain predicate (e.g. "what is X", "arxiv
//! paper on Y"), a single domain-specific provider is queried and
//! its results are merged in.

use std::sync::Arc;

use super::arxiv::ArxivProvider;
use super::duckduckgo::DuckDuckGoProvider;
use super::github::GitHubProvider;
use super::google_news::GoogleNewsProvider;
use super::hn_algolia::HnAlgoliaProvider;
use super::mojeek::MojeekProvider;
use super::openalex::OpenAlexProvider;
use super::providers::{BraveProvider, ExaProvider, KagiProvider, SerpApiProvider, TavilyProvider};
use super::pubmed::PubMedProvider;
use super::routing::{classify_query, QueryKind};
use super::types::{Domain, SearchError, SearchHit, SearchProvider, Specificity};
use super::wikipedia::WikipediaProvider;

pub struct SearchProviderRegistry {
    general: Vec<Arc<dyn SearchProvider>>,
    fallbacks: Vec<Arc<dyn SearchProvider>>,
    domain: Vec<Arc<dyn SearchProvider>>,
}

impl SearchProviderRegistry {
    /// Build a registry from environment variables. Only providers
    /// with what they need to operate are included.
    pub fn from_env() -> Self {
        let candidates: Vec<Arc<dyn SearchProvider>> = vec![
            Arc::new(ExaProvider::from_env()),
            Arc::new(TavilyProvider::from_env()),
            Arc::new(BraveProvider::from_env()),
            Arc::new(KagiProvider::from_env()),
            Arc::new(SerpApiProvider::from_env()),
        ];
        let general: Vec<Arc<dyn SearchProvider>> = candidates
            .into_iter()
            .filter(|p| p.is_configured())
            .collect();
        let fallbacks: Vec<Arc<dyn SearchProvider>> = vec![
            Arc::new(DuckDuckGoProvider::new()),
            Arc::new(MojeekProvider::new()),
        ];
        let domain: Vec<Arc<dyn SearchProvider>> = vec![
            Arc::new(WikipediaProvider::new()),
            Arc::new(ArxivProvider::new()),
            Arc::new(OpenAlexProvider::new()),
            Arc::new(PubMedProvider::new()),
            Arc::new(HnAlgoliaProvider::new()),
            Arc::new(GoogleNewsProvider::new()),
            Arc::new(GitHubProvider::new()),
        ];
        Self {
            general,
            fallbacks,
            domain,
        }
    }

    /// Build a registry with a custom set of providers. Useful in
    /// tests.
    pub fn with_providers(
        general: Vec<Arc<dyn SearchProvider>>,
        fallbacks: Vec<Arc<dyn SearchProvider>>,
        domain: Vec<Arc<dyn SearchProvider>>,
    ) -> Self {
        Self {
            general,
            fallbacks,
            domain,
        }
    }

    /// True if at least one provider is configured (key or no-key).
    pub fn has_any(&self) -> bool {
        !self.general.is_empty() || !self.fallbacks.is_empty() || !self.domain.is_empty()
    }

    /// Human-readable list of configured providers, e.g. for
    /// the tool's `description()`.
    pub fn describe_configured(&self) -> String {
        let mut names = Vec::new();
        names.extend(self.general.iter().map(|p| p.name().to_string()));
        names.extend(self.fallbacks.iter().map(|p| p.name().to_string()));
        names.extend(self.domain.iter().map(|p| p.name().to_string()));
        if names.is_empty() {
            "no provider configured".to_string()
        } else {
            names.join(", ")
        }
    }

    /// Run a search.
    ///
    /// - `query` is the user query.
    /// - `num_results` is the upper bound on hits to return.
    /// - `provider_hint` is an optional provider name (e.g.
    ///   `"wikipedia"`) that biases the dispatch. If unset, the
    ///   registry picks based on the query shape and the chain order.
    pub async fn search(
        &self,
        query: &str,
        num_results: usize,
        provider_hint: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        if query.trim().is_empty() {
            return Err(SearchError::Transport("empty query".into()));
        }
        let limit = num_results.max(1);
        let mut collected: Vec<SearchHit> = Vec::new();

        // 1. Key-based general providers.
        if let Some(hinted) = pick_one(&self.general, provider_hint) {
            if let Ok(hits) = hinted.search(query, limit).await {
                if !hits.is_empty() {
                    collected.extend(hits);
                }
            }
        } else {
            for p in &self.general {
                match p.search(query, limit).await {
                    Ok(hits) if !hits.is_empty() => {
                        collected.extend(hits);
                        break;
                    }
                    _ => continue,
                }
            }
        }

        // 2. No-key fallbacks: try DDG, then Mojeek.
        if collected.len() < limit {
            let mut fallback_tried = false;
            for p in &self.fallbacks {
                match p.search(query, limit).await {
                    Ok(hits) if !hits.is_empty() => {
                        collected.extend(hits);
                        fallback_tried = true;
                        break;
                    }
                    Ok(_) => {
                        fallback_tried = true;
                        continue;
                    }
                    Err(_) => continue,
                }
            }
            if !fallback_tried && self.fallbacks.is_empty() {
                // No fallbacks at all.
            }
        }

        // 3. Domain providers — only if results are thin AND the
        //    query shape matches a domain predicate.
        if collected.len() < 2 {
            let kind = match provider_hint {
                Some(p) => name_to_kind(p),
                None => classify_query(query),
            };
            if let Some(target) = pick_domain(&self.domain, kind) {
                if let Ok(hits) = target.search(query, limit).await {
                    collected.extend(hits);
                }
            }
        }

        if collected.is_empty() {
            return Err(SearchError::Empty);
        }
        collected.truncate(limit);
        Ok(collected)
    }
}

fn pick_one(
    providers: &[Arc<dyn SearchProvider>],
    hint: Option<&str>,
) -> Option<Arc<dyn SearchProvider>> {
    let hint = hint?;
    providers.iter().find(|p| p.name() == hint).cloned()
}

fn name_to_kind(name: &str) -> QueryKind {
    match name {
        "wikipedia" => QueryKind::Encyclopedic,
        "arxiv" | "openalex" => QueryKind::Academic,
        "pubmed" => QueryKind::Biomedical,
        "google_news" => QueryKind::News,
        "hn_algolia" => QueryKind::TechDiscourse,
        "github" => QueryKind::Code,
        _ => QueryKind::General,
    }
}

fn pick_domain(
    providers: &[Arc<dyn SearchProvider>],
    kind: QueryKind,
) -> Option<Arc<dyn SearchProvider>> {
    let want = match kind {
        QueryKind::General => return None,
        QueryKind::Encyclopedic => Domain::Encyclopedic,
        QueryKind::Academic => Domain::Academic,
        QueryKind::Biomedical => Domain::Biomedical,
        QueryKind::News => Domain::News,
        QueryKind::TechDiscourse => Domain::TechDiscourse,
        QueryKind::Code => Domain::Code,
    };
    providers
        .iter()
        .find(|p| matches!(p.specificity(), Specificity::Domain(d) if d == want))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// A test-only provider that always returns the configured hits.
    struct StubProvider {
        name: &'static str,
        hits: Vec<SearchHit>,
        specificity: Specificity,
    }

    #[async_trait]
    impl SearchProvider for StubProvider {
        fn name(&self) -> &'static str {
            self.name
        }
        fn is_configured(&self) -> bool {
            true
        }
        fn specificity(&self) -> Specificity {
            self.specificity
        }
        async fn search(&self, _q: &str, n: usize) -> Result<Vec<SearchHit>, SearchError> {
            Ok(self.hits.iter().take(n).cloned().collect())
        }
    }

    fn hit(title: &str) -> SearchHit {
        SearchHit {
            title: title.into(),
            url: format!("https://example.com/{title}"),
            snippet: String::new(),
            source: "stub".into(),
        }
    }

    #[test]
    fn empty_registry_returns_empty() {
        let reg = SearchProviderRegistry::with_providers(vec![], vec![], vec![]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(reg.search("test", 5, None));
        assert!(matches!(res, Err(SearchError::Empty)));
    }

    #[test]
    fn general_provider_results_are_returned() {
        let p: Arc<dyn SearchProvider> = Arc::new(StubProvider {
            name: "stub_general",
            hits: vec![hit("a"), hit("b")],
            specificity: Specificity::General,
        });
        let reg = SearchProviderRegistry::with_providers(vec![p], vec![], vec![]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(reg.search("test", 5, None)).expect("ok");
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].title, "a");
    }

    #[test]
    fn thin_results_trigger_domain_provider() {
        let thin: Arc<dyn SearchProvider> = Arc::new(StubProvider {
            name: "thin",
            hits: vec![hit("only")],
            specificity: Specificity::General,
        });
        let wiki: Arc<dyn SearchProvider> = Arc::new(StubProvider {
            name: "wikipedia",
            hits: vec![hit("entity")],
            specificity: Specificity::Domain(Domain::Encyclopedic),
        });
        let reg = SearchProviderRegistry::with_providers(vec![], vec![], vec![wiki]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt
            .block_on(reg.search("what is rust", 5, None))
            .expect("ok");
        // Domain provider should be queried because result is thin AND
        // query shape matches Encyclopedic.
        assert!(res.iter().any(|h| h.title == "entity"));
        let _ = thin;
    }

    #[test]
    fn describe_includes_all_configured() {
        let p: Arc<dyn SearchProvider> = Arc::new(StubProvider {
            name: "stub",
            hits: vec![],
            specificity: Specificity::General,
        });
        let reg = SearchProviderRegistry::with_providers(vec![p.clone()], vec![p], vec![]);
        let s = reg.describe_configured();
        assert!(s.contains("stub"));
    }
}
