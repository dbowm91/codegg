---
name: search
description: Pluggable no-key web search providers used by the `websearch` tool and `research` tool
version: 1.0.0
tags:
  - search
  - websearch
  - research
  - providers
  - ssrf
---

# Search Provider System

The `src/search/` module provides a pluggable, key-optional web search registry that backs the `websearch` tool and the `research` deep-research tool. It is designed to be useful out of the box (no API keys required) while still allowing higher-quality providers when keys are configured.

## Module Layout

| File | Purpose |
|------|---------|
| `src/search/mod.rs` | Re-exports `SearchProvider`, `SearchHit`, `SearchError`, `SearchProviderRegistry`, `classify_query` |
| `src/search/types.rs` | `SearchError`, `SearchHit`, `Specificity` enum, `SearchProvider` trait |
| `src/search/registry.rs` | `SearchProviderRegistry::from_env()`, fallback chain, `search()` |
| `src/search/routing.rs` | `QueryKind`, `classify_query()` (biomedical / academic / code / news / tech_discourse) |
| `src/search/providers.rs` | Exa, Tavily, Brave, Kagi, SerpAPI (key-based) |
| `src/search/duckduckgo.rs` | DDG HTML scraper, `parse_duckduckgo_html`, `resolve_uddg` |
| `src/search/mojeek.rs` | Mojeek HTML scraper, `parse_mojeek_html` |
| `src/search/wikipedia.rs` | Wikipedia Action API `list=search` |
| `src/search/arxiv.rs` | arXiv Atom XML parser |
| `src/search/openalex.rs` | OpenAlex `api.openalex.org/works?search=` |
| `src/search/pubmed.rs` | PubMed E-utilities esearch+esummary two-step |
| `src/search/hn_algolia.rs` | Hacker News Algolia search |
| `src/search/google_news.rs` | Google News RSS parser |
| `src/search/github.rs` | GitHub repo search |
| `src/ssrf.rs` | SSRF validation (reused from `src/security/ssrf.rs`) |

## Provider Tiers

The registry is built in three layers:

1. **Key-based general providers** — `ExaProvider`, `TavilyProvider`, `BraveProvider`, `KagiProvider`, `SerpAPIProvider`. Activated via env vars (`EXA_API_KEY`, `TAVILY_API_KEY`, etc.).
2. **No-key HTML fallbacks** — `DuckDuckGoProvider`, `MojeekProvider`. Always available.
3. **Domain providers** — `WikipediaProvider`, `ArxivProvider`, `OpenAlexProvider`, `PubmedProvider`, `HackerNewsProvider`, `GoogleNewsProvider`, `GithubProvider`. Always available; only used when general results are thin AND the query shape matches a domain predicate.

Routing order: `keys → DDG → Mojeek`. If the general chain returns < 3 results AND the query classifies to a specific domain, the registry falls back to a domain provider.

## Adding a New Provider

```rust
// 1. Define your provider in src/search/<name>.rs
pub struct MyProvider;

impl SearchProvider for MyProvider {
    fn name(&self) -> &'static str { "my_provider" }
    fn specificity(&self) -> Specificity { Specificity::General }
    async fn search(&self, query: &str, n: usize) -> Result<Vec<SearchHit>, SearchError> {
        // ...
    }
}

// 2. Add to src/search/registry.rs
fn build_my_provider() -> Option<Arc<dyn SearchProvider>> { ... }

// 3. Push into the appropriate Vec in SearchProviderRegistry::from_env
```

Each provider should:
- Return `SearchError::Empty` when no hits (caller will try next provider).
- Use `ssrf::validate_host_ip` + `revalidate_dns` for outbound URLs to prevent SSRF.
- Have at least one test under a `mod tests` block in its file.

## SSRF Protection

All HTTP egress goes through `src/security/ssrf.rs`:

```rust
use crate::security::ssrf::{revalidate_dns, validate_host_ip};

let host = "duckduckgo.com";
validate_host_ip(host)?;           // IP-based block list (RFC 6892)
revalidate_dns(host, 443).await?;   // Resolve at call time to defeat DNS rebinding
```

If either check fails, the provider returns `SearchError::InvalidRequest` and the registry moves to the next fallback.

## Query Classification

`classify_query()` in `src/search/routing.rs` uses keyword predicates to assign a `QueryKind`:

```rust
pub enum QueryKind { General, Biomedical, Academic, Code, News, TechDiscourse }
```

Biomedical hints include drug/condition terms (`metformin`, `side effect`, `pubmed`). Academic hints include arXiv/doi/cite. News hints include `news`/`latest`/`breaking`. Tech-discourse includes `r/lobsters`, `r/programming`. The classification is *additive* — it only narrows the provider set when the general chain is thin.

## Integration with Tools

- **`websearch` tool** (`src/tool/websearch.rs`): calls `SearchProviderRegistry::from_env().search(q, n)`. The tool description is generated dynamically based on `registry.has_any()` to reflect available providers.
- **`research` tool** (`src/tool/research.rs`): wraps `ResearchService::answer_for_agent` for deep research. The service may invoke `websearch` and `webfetch` internally.
- **`ModelFlags::search_provider_available`** (in `src/agent/loop.rs`): used by `assemble_system_prompt_with_profile` to gate the `websearch_contract()` prompt section.

## Testing

Each provider has at least one unit test in a `mod tests` block. The full suite lives in `src/search/*/mod tests` plus:

```bash
cargo test --lib search::
```

For end-to-end (live network) tests, see `src/search/providers.rs` doc comments — they are gated behind feature flags or env-var presence to avoid CI flakiness.

## Environment Variables

| Variable | Provider | Notes |
|----------|----------|-------|
| `EXA_API_KEY` | Exa | |
| `TAVILY_API_KEY` | Tavily | |
| `BRAVE_API_KEY` | Brave | |
| `KAGI_API_KEY` | Kagi | |
| `SERP_API_KEY` | SerpAPI | |
| `WIKIPEDIA_USER_AGENT` | Wikipedia | Required by their API policy |
| `OPENALEX_MAILTO` | OpenAlex | Polite-pool email |
| `PUBMED_TOOL` / `PUBMED_EMAIL` | PubMed | Per NCBI guidance |

All key-based providers are optional. If none are set, DDG + Mojeek are the defaults.
