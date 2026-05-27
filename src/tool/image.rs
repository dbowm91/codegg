use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use crate::error::ToolError;
use crate::security::ssrf::{revalidate_dns, validate_host_ip};
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub url: Option<String>,
    pub b64_json: Option<String>,
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub created: u64,
    pub data: Vec<ImageData>,
}

pub struct ImageTool {
    client: Client,
    api_key: Option<String>,
    base_url: String,
}

impl ImageTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            base_url: "https://api.openai.com/v1/images/generations".to_string(),
        }
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
}

impl Default for ImageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ImageTool {
    fn name(&self) -> &str {
        "image"
    }

    fn description(&self) -> &str {
        "Generate images using OpenAI's DALL-E model"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "A text description of the desired image(s). The maximum length is 4000 characters for DALL-E 3."
                },
                "model": {
                    "type": "string",
                    "description": "The model to use for image generation (default: dall-e-3)"
                },
                "size": {
                    "type": "string",
                    "description": "The size of the generated images (e.g., 1024x1024 for DALL-E 3)"
                },
                "quality": {
                    "type": "string",
                    "description": "The quality of the image (standard or hd for DALL-E 3)"
                },
                "n": {
                    "type": "number",
                    "description": "The number of images to generate (default: 1)"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ToolError::Execution("OPENAI_API_KEY not set".to_string()))?;

        let prompt = input["prompt"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'prompt' parameter".to_string()))?;

        let parsed_url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| ToolError::Execution(format!("invalid base_url: {}", e)))?;

        let host = parsed_url
            .host_str()
            .ok_or_else(|| ToolError::Execution("base_url must have a host".to_string()))?;
        let port = parsed_url.port().unwrap_or(443);
        let validated_ips =
            validate_host_ip(host, port).map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let mut body = serde_json::json!({
            "prompt": prompt,
            "model": input["model"].as_str().unwrap_or("dall-e-3"),
            "n": input["n"].as_u64().unwrap_or(1) as usize,
        });

        if let Some(size) = input["size"].as_str() {
            body["size"] = serde_json::json!(size);
        }

        if let Some(quality) = input["quality"].as_str() {
            body["quality"] = serde_json::json!(quality);
        }

        revalidate_dns(host, port, &validated_ips)
            .map_err(|e| ToolError::Execution(format!("SSRF protection: {}", e)))?;

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", api_key))
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

        let json: ImageResponse = response
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("parse failed: {}", e)))?;

        let mut output = format!("Generated {} image(s):\n\n", json.data.len());
        for (i, image) in json.data.iter().enumerate() {
            output.push_str(&format!("Image {}:\n", i + 1));
            if let Some(url) = &image.url {
                output.push_str(&format!("  URL: {}\n", url));
            }
            if let Some(b64) = &image.b64_json {
                output.push_str(&format!("  [base64 data: {} bytes]\n", b64.len()));
            }
            if let Some(revised_prompt) = &image.revised_prompt {
                output.push_str(&format!("  Revised prompt: {}\n", revised_prompt));
            }
            output.push('\n');
        }

        Ok(output)
    }
}
