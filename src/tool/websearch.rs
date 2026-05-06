use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ToolError;
use crate::security::ssrf::{revalidate_dns, validate_host_ip};
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct WebSearchTool {
    client: Client,
    api_key: Option<String>,
    base_url: String,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            api_key: std::env::var("EXA_API_KEY").ok(),
            base_url: "https://api.exa.ai/search".to_string(),
        }
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    async fn search_exa(&self, query: &str, num_results: usize) -> Result<String, ToolError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ToolError::Execution("EXA_API_KEY not set".to_string()))?;

        let parsed_url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ToolError::Execution(format!("invalid base_url: {}", e)))?;

        let host = parsed_url
            .host_str()
            .ok_or_else(|| ToolError::Execution("base_url must have a host".to_string()))?;
        let port = parsed_url.port().unwrap_or(443);
        let validated_ips = validate_host_ip(host, port)
            .map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let body = json!({
            "query": query,
            "numResults": num_results,
            "type": "auto",
            "livecrawl": "fallback",
        });

        revalidate_dns(host, port, &validated_ips)
            .map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let response = self
            .client
            .post(&self.base_url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ToolError::Execution(format!(
                "API error {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("parse failed: {}", e)))?;

        let results = json["results"]
            .as_array()
            .ok_or_else(|| ToolError::Execution("no results in response".to_string()))?;

        if results.is_empty() {
            return Ok(format!("No results found for '{}'", query));
        }

        let mut output = format!("Search results for '{}':\n\n", query);
        for (i, result) in results.iter().enumerate() {
            let title = result["title"].as_str().unwrap_or("Untitled");
            let url = result["url"].as_str().unwrap_or("");
            let snippet = result["text"].as_str().unwrap_or("");

            output.push_str(&format!(
                "{}. {}\n   URL: {}\n   {}\n\n",
                i + 1,
                title,
                url,
                snippet
            ));
        }

        Ok(output)
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Search the web for information using Exa AI"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "number",
                    "description": "Number of results to return (default: 8)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;

        let num_results = input["num_results"].as_u64().unwrap_or(8) as usize;

        self.search_exa(query, num_results).await
    }
}
