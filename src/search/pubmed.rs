//! PubMed E-utilities search (no API key, requires `tool` + `email`).
//!
//! Two-step flow: `esearch` returns PMIDs; `esummary` (batched) returns
//! titles and details. NCBI asks for `tool=<name>&email=<email>`; we
//! use fixed placeholder values. For 10 req/s, register a real key.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ESEARCH: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi";
const ESUMMARY: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi";
const TOOL: &str = "codegg-websearch";
const EMAIL: &str = "codegg-research@example.invalid";

pub struct PubMedProvider {
    client: Client,
}

impl PubMedProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("codegg-websearch/1.0 (research use; pubmed)")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for PubMedProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for PubMedProvider {
    fn name(&self) -> &'static str {
        "pubmed"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::Biomedical)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 20);

        // Step 1: esearch → list of PMIDs.
        let resp = self
            .client
            .get(ESEARCH)
            .query(&[
                ("db", "pubmed"),
                ("term", query),
                ("retmax", &limit.to_string()),
                ("retmode", "json"),
                ("tool", TOOL),
                ("email", EMAIL),
            ])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::Http {
                status: status.as_u16(),
                body,
            });
        }
        #[derive(Deserialize)]
        struct ESearchResp {
            esearchresult: ESearchResult,
        }
        #[derive(Deserialize)]
        struct ESearchResult {
            #[serde(default)]
            idlist: Vec<String>,
        }
        let r: ESearchResp = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        if r.esearchresult.idlist.is_empty() {
            return Err(SearchError::Empty);
        }
        let pmids = r.esearchresult.idlist;

        // Step 2: esummary (batch) → titles.
        let resp2 = self
            .client
            .get(ESUMMARY)
            .query(&[
                ("db", "pubmed"),
                ("id", &pmids.join(",")),
                ("retmode", "json"),
                ("tool", TOOL),
                ("email", EMAIL),
            ])
            .send()
            .await?;
        if !resp2.status().is_success() {
            return Err(SearchError::Empty);
        }
        #[derive(Deserialize)]
        struct ESummaryResp {
            result: std::collections::HashMap<String, serde_json::Value>,
        }
        let s: ESummaryResp = resp2.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        let mut out = Vec::new();
        for pmid in pmids {
            if let Some(item) = s.result.get(&pmid) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(untitled)")
                    .to_string();
                let url = format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/");
                let snippet = item
                    .get("sortfirstauthor")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push(SearchHit {
                    title,
                    url,
                    snippet,
                    source: "pubmed".into(),
                });
            }
        }
        if out.is_empty() {
            Err(SearchError::Empty)
        } else {
            Ok(out)
        }
    }
}
