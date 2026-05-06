use crate::error::ToolError;
use crate::security::ssrf::{revalidate_dns, validate_host_ip};
use crate::tool::Tool;
use async_trait::async_trait;
use serde::Deserialize;

const MAX_QUERY_LENGTH: usize = 10000;

#[derive(Debug, Deserialize)]
struct CodeSearchInput {
    query: String,
    #[serde(default = "default_tokens")]
    tokens_num: usize,
}

fn default_tokens() -> usize {
    5000
}

pub struct CodeSearchTool;

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        "Search for relevant code examples, library docs, and SDK patterns using Exa Code API."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for code context"
                },
                "tokens_num": {
                    "type": "number",
                    "description": "Number of tokens to return (1000-50000, default: 5000)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: CodeSearchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid codesearch input: {e}")))?;

        if parsed.query.len() > MAX_QUERY_LENGTH {
            return Err(ToolError::Execution(format!(
                "query exceeds maximum length of {} characters",
                MAX_QUERY_LENGTH
            )));
        }

        let sanitized: String = parsed
            .query
            .chars()
            .filter(|&c| {
                !c.is_control() && c != '\'' && c != '"' && c != ';' && c != '\\' && c != '\0'
            })
            .collect();

        if sanitized.is_empty() {
            return Err(ToolError::Execution(
                "query contains no valid characters".to_string(),
            ));
        }

        let api_key = std::env::var("EXA_API_KEY")
            .or_else(|_| std::env::var("EXA_CODE_API_KEY"))
            .map_err(|_| ToolError::Execution("EXA_API_KEY not set".to_string()))?;

        let api_url = "https://api.exa.ai/code";
        let parsed_url = reqwest::Url::parse(api_url)
            .map_err(|e| ToolError::Execution(format!("invalid API URL: {}", e)))?;

        let host = parsed_url
            .host_str()
            .ok_or_else(|| ToolError::Execution("API URL must have a host".to_string()))?;
        let port = parsed_url.port().unwrap_or(443);
        let validated_ips = validate_host_ip(host, port)
            .map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::Execution(format!("failed to create client: {e}")))?;

        let body = serde_json::json!({
            "query": sanitized,
            "tokensNum": parsed.tokens_num.clamp(1000, 50000),
        });

        revalidate_dns(host, port, &validated_ips)
            .map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let resp = client
            .post(api_url)
            .header("x-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolError::Execution(format!("API error {status}: {text}")));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("failed to read response: {e}")))?;

        Ok(text)
    }
}
