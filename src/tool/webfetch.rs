use async_trait::async_trait;
use html2text::from_read;
use serde_json::json;
use std::time::Duration;

use crate::error::ToolError;
use crate::security::ssrf::{revalidate_dns, validate_host_ip, validate_url_host};
use crate::tool::Tool;

const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024; // 5MB
const IMAGE_CONTENT_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
    "image/bmp",
];

pub struct WebFetchTool {
    client: reqwest::Client,
    timeout: Duration,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self.client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content as markdown. Handles Cloudflare challenges, images as base64, and 5MB size limits."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "max_length": {
                    "type": "number",
                    "description": "Maximum characters to return (default: 10000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'url' parameter".to_string()))?;

        let max_length = input["max_length"].as_u64().unwrap_or(10_000) as usize;

        let host = validate_url_host(url).map_err(ToolError::Execution)?;

        let parsed_url = reqwest::Url::parse(url)
            .map_err(|_| ToolError::Execution("invalid URL".to_string()))?;
        let port = parsed_url.port().unwrap_or_else(|| {
            if parsed_url.scheme() == "https" {
                443
            } else {
                80
            }
        });
        let validated_ips = validate_host_ip(&host, port).map_err(ToolError::Execution)?;

        revalidate_dns(&host, port, &validated_ips).map_err(ToolError::Execution)?;

        let response = self
            .client
            .get(url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (compatible; Codegg/1.0; +https://codegg.ai)",
            )
            .send()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if status.as_u16() == 403 || status.as_u16() == 503 {
            let retry_url = url::Url::parse(url)
                .map_err(|e| ToolError::Execution(format!("invalid retry URL: {e}")))?;
            let retry_host = retry_url
                .host_str()
                .ok_or_else(|| ToolError::Execution("retry URL must have a host".to_string()))?;
            let retry_port = retry_url.port().unwrap_or_else(|| {
                if retry_url.scheme() == "https" {
                    443
                } else {
                    80
                }
            });
            revalidate_dns(retry_host, retry_port, &validated_ips).map_err(ToolError::Execution)?;

            let retry_resp = self
                .client
                .get(url)
                .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
                .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.5")
                .send()
                .await
                .map_err(|e| ToolError::Execution(format!("Cloudflare retry failed: {e}")))?;

            let retry_content_type = retry_resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            return self
                .process_response(retry_resp, &retry_content_type, max_length)
                .await;
        }

        self.process_response(response, &content_type, max_length)
            .await
    }
}

impl WebFetchTool {
    async fn process_response(
        &self,
        response: reqwest::Response,
        content_type: &str,
        max_length: usize,
    ) -> Result<String, ToolError> {
        let is_image = IMAGE_CONTENT_TYPES
            .iter()
            .any(|ct| content_type.starts_with(ct));

        if is_image {
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?;

            if bytes.len() > MAX_RESPONSE_SIZE {
                return Err(ToolError::Execution(format!(
                    "image response exceeds 5MB limit ({} bytes)",
                    bytes.len()
                )));
            }

            let encoded =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
            return Ok(format!("[{content_type} base64 attachment]\n{encoded}"));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if bytes.len() > MAX_RESPONSE_SIZE {
            return Err(ToolError::Execution(format!(
                "response exceeds 5MB limit ({} bytes)",
                bytes.len()
            )));
        }

        let result = if content_type.contains("html") {
            from_read(&bytes[..], 80)
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string())
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        };

        if result.len() > max_length {
            Ok(format!("{}... [truncated]", &result[..max_length]))
        } else {
            Ok(result)
        }
    }
}
