use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    /// API prefix contract: `base_url` is the versioned API prefix
    /// (for example `https://api.openai.com/v1`), never the host root.
    /// The chat-completions endpoint is `{base_url}/chat/completions`
    /// composed exactly once by [`chat_completions_url`].
    pub api_key: String,
    pub base_url: String,
    pub provider_id: String,
    pub provider_name: String,
    pub requires_org_header: bool,
    pub organization: Option<String>,
    pub omit_stream_options: bool,
    pub tool_choice: crate::openai_compatible::ToolChoice,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: false,
            organization: None,
            omit_stream_options: false,
            tool_choice: crate::openai_compatible::ToolChoice::Auto,
        }
    }
}

impl OpenAiConfig {
    pub fn default_with_key(api_key: String) -> Self {
        Self {
            api_key,
            ..Default::default()
        }
    }

    pub fn openai(api_key: String) -> Self {
        Self {
            api_key,
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: true,
            ..Default::default()
        }
    }

    /// Exact chat-completions endpoint for this configuration.
    pub fn chat_completions_url(&self) -> Result<String, ProviderError> {
        chat_completions_url(&self.base_url)
    }
}

/// Compose the native OpenAI chat-completions endpoint from an API prefix.
///
/// Contract: `base_url` is the versioned API prefix
/// (for example `https://api.openai.com/v1`). The result appends exactly one
/// `/chat/completions` segment after trimming trailing slashes, preserving
/// any explicitly configured non-default path prefix.
///
/// Rejects empty input, control characters, and values that cannot serve as
/// an HTTP(S) endpoint prefix. Never logs credentials; the error carries only
/// a stable code.
pub fn chat_completions_url(base_url: &str) -> Result<String, ProviderError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(ProviderError::api(
            "invalid_endpoint",
            "native OpenAI base URL is empty or contains control characters",
        ));
    }
    // Require an http(s) prefix so a bare hostname or a non-URL value fails
    // before network I/O rather than composing a misleading relative URL.
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(ProviderError::api(
            "invalid_endpoint",
            "native OpenAI base URL must start with http:// or https://",
        ));
    }
    let without_slash = trimmed.trim_end_matches('/');
    if without_slash.is_empty() {
        return Err(ProviderError::api(
            "invalid_endpoint",
            "native OpenAI base URL is empty",
        ));
    }
    Ok(format!("{without_slash}/chat/completions"))
}

#[derive(Clone)]
pub struct OpenAiProvider {
    cfg: OpenAiConfig,
    client: eggfetch_core::Client,
}

impl OpenAiProvider {
    pub fn new(cfg: OpenAiConfig) -> Self {
        Self {
            cfg,
            client: create_http_client(),
        }
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        self.try_build_body(req).unwrap_or_else(|error| {
            tracing::error!("shared OpenAI request encoding failed: {}", error);
            serde_json::Value::Null
        })
    }

    fn try_build_body(&self, req: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        crate::wire::encode_openai_chat(
            req,
            Some(&self.cfg.tool_choice),
            !self.cfg.omit_stream_options,
        )
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.cfg.provider_id
    }

    fn name(&self) -> &str {
        &self.cfg.provider_name
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.try_build_body(req)?;
        let url = self.cfg.chat_completions_url()?;
        let api_key = self.cfg.api_key.clone();
        let client = self.client.clone();
        let requires_org = self.cfg.requires_org_header;
        let org = self.cfg.organization.clone();

        let mut req_builder = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("authorization", &format!("Bearer {}", api_key))
            .header("content-type", "application/json");

        if requires_org {
            if let Some(ref org_id) = org {
                req_builder = req_builder.header("OpenAI-Organization", org_id);
            }
        }

        let mut resp = req_builder
            .json(&body)
            .map_err(|e| ProviderError::api("serialization", e.to_string()))?
            .send()
            .await
            .map_err(ProviderError::from)?;

        if resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(resp.headers()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: {}", status, err_text),
            ));
        }

        let stream = resp.bytes_stream().map_err(ProviderError::from)?;
        Ok(crate::wire::openai_chat_stream(
            stream,
            req.context.wire_policy.clone(),
            None,
        ))
    }

    /// Discover models upstream. There is no compiled-in catalog: the shared
    /// compatible provider owns the bounded, profile-driven discovery call, and
    /// an unreachable `/models` yields an empty catalog rather than fiction.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let discovery = crate::openai_compatible::OpenAiCompatibleProvider::simple_with_credential(
            &self.cfg.provider_id,
            &self.cfg.provider_name,
            crate::auth_types::Credential::api_key(self.cfg.api_key.clone()),
            &self.cfg.base_url,
        );
        discovery.models().await
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentPart, Message, ProviderRequestContext};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Arc};
    use std::thread;

    struct CapturedRequest {
        request_line: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    fn spawn_capture_server(
        expected_requests: usize,
    ) -> (
        String,
        mpsc::Receiver<CapturedRequest>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let address = listener.local_addr().expect("capture server address");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("accept provider request");
                let request = read_request(&mut stream);
                tx.send(request).expect("send captured request");
                let response_body =
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                )
                .expect("write capture response headers");
                stream
                    .write_all(response_body)
                    .expect("write capture response body");
            }
        });
        (format!("http://{address}"), rx, handle)
    }

    fn read_request(stream: &mut TcpStream) -> CapturedRequest {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set capture timeout");
        let mut raw = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 4096];
            let count = stream.read(&mut chunk).expect("read provider request");
            assert!(count > 0, "provider closed before sending request");
            raw.extend_from_slice(&chunk[..count]);
            if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header_text = String::from_utf8_lossy(&raw[..header_end]).into_owned();
        let request_line = header_text.lines().next().unwrap_or_default().to_string();
        let content_length = header_text
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .or_else(|| line.strip_prefix("content-length:"))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while raw.len() - header_end < content_length {
            let mut chunk = [0u8; 4096];
            let count = stream.read(&mut chunk).expect("read provider request body");
            assert!(count > 0, "provider closed before sending request body");
            raw.extend_from_slice(&chunk[..count]);
        }
        let mut headers = Vec::new();
        for line in header_text.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        CapturedRequest {
            request_line,
            headers,
            body: String::from_utf8_lossy(&raw[header_end..header_end + content_length]).into(),
        }
    }

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            }],
            model: "gpt-4o".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: ProviderRequestContext {
                session_id: None,
                ..Default::default()
            },
        }
    }

    fn header_values<'a>(request: &'a CapturedRequest, name: &str) -> Vec<&'a str> {
        request
            .headers
            .iter()
            .filter(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    #[test]
    fn api_prefix_composer_appends_chat_completions_exactly_once() {
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1").expect("valid prefix"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/").expect("trailing slash"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://proxy.example/custom/prefix").expect("custom prefix"),
            "https://proxy.example/custom/prefix/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://proxy.example/custom/prefix/").expect("custom slash"),
            "https://proxy.example/custom/prefix/chat/completions"
        );
    }

    #[test]
    fn api_prefix_composer_rejects_invalid_endpoints_without_secrets() {
        for invalid in [
            "",
            "   ",
            "api.openai.com/v1",
            "ftp://api.openai.com/v1",
            "https://api.openai.com/v1\u{7}",
        ] {
            let error = match chat_completions_url(invalid) {
                Ok(url) => panic!("invalid base URL must fail, got {url}"),
                Err(error) => error,
            };
            assert!(
                matches!(error, ProviderError::Api { ref code, .. } if code == "invalid_endpoint"),
                "unexpected error for {invalid:?}: {error:?}"
            );
            assert!(!format!("{error:?}").contains("sk-secret"));
        }
    }

    #[test]
    fn default_config_uses_versioned_api_prefix_once() {
        let config = OpenAiConfig::default();
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(
            config.chat_completions_url().expect("default URL"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert!(!config
            .chat_completions_url()
            .expect("default URL")
            .contains("/v1/v1"));
    }

    #[test]
    fn setup_catalog_openai_constant_matches_native_default() {
        assert_eq!(
            crate::setup_catalog::OPENAI_BASE_URL,
            OpenAiConfig::default().base_url
        );
    }

    #[tokio::test]
    async fn native_openai_capture_server_emits_single_version_prefix() {
        use crate::Provider as _;
        let (base, rx, server) = spawn_capture_server(1);
        let prefix = format!("{base}/v1");
        let config = OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: prefix.clone(),
            ..OpenAiConfig::default()
        };
        let provider = OpenAiProvider::new(config);
        let _stream = provider.stream(&request()).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert!(
            captured.request_line.contains("POST /v1/chat/completions "),
            "unexpected request line: {}",
            captured.request_line
        );
        assert!(
            !captured.request_line.contains("/v1/v1"),
            "duplicated version prefix: {}",
            captured.request_line
        );
    }

    #[tokio::test]
    async fn native_openai_trailing_slash_does_not_duplicate_separator() {
        use crate::Provider as _;
        let (base, rx, server) = spawn_capture_server(1);
        let config = OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: format!("{base}/v1/"),
            ..OpenAiConfig::default()
        };
        let provider = OpenAiProvider::new(config);
        let _stream = provider.stream(&request()).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert!(
            captured.request_line.contains("POST /v1/chat/completions "),
            "unexpected request line: {}",
            captured.request_line
        );
        assert!(!captured.request_line.contains("//chat/completions"));
    }

    #[tokio::test]
    async fn native_openai_custom_prefix_is_preserved() {
        use crate::Provider as _;
        let (base, rx, server) = spawn_capture_server(1);
        let config = OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: format!("{base}/custom/prefix"),
            ..OpenAiConfig::default()
        };
        let provider = OpenAiProvider::new(config);
        let _stream = provider.stream(&request()).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert!(
            captured
                .request_line
                .contains("POST /custom/prefix/chat/completions "),
            "unexpected request line: {}",
            captured.request_line
        );
    }

    #[tokio::test]
    async fn native_openai_organization_header_behavior_is_unchanged() {
        use crate::Provider as _;
        let (base, rx, server) = spawn_capture_server(1);
        let config = OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: format!("{base}/v1"),
            requires_org_header: true,
            organization: Some("org-123".to_string()),
            ..OpenAiConfig::default()
        };
        let provider = OpenAiProvider::new(config);
        let _stream = provider.stream(&request()).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured, "openai-organization"),
            vec!["org-123"]
        );
        assert_eq!(
            header_values(&captured, "authorization"),
            vec!["Bearer test-key"]
        );
        assert!(!captured.body.is_empty(), "chat body must still be sent");
    }

    #[tokio::test]
    async fn native_openai_invalid_endpoint_fails_before_network_io() {
        use crate::Provider as _;
        let config = OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: "not-a-url".to_string(),
            ..OpenAiConfig::default()
        };
        let provider = OpenAiProvider::new(config);
        let error = match provider.stream(&request()).await {
            Ok(_) => panic!("invalid endpoint must fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::Api { ref code, .. } if code == "invalid_endpoint"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn native_source_does_not_reintroduce_duplicated_version_prefix() {
        let source = include_str!("openai.rs");
        // Old defect composed `{base_url}/v1/chat/completions` on top of an
        // already-versioned prefix. Match the old format prefix rather than
        // the plain path so this guard does not match its own expectation.
        let old_format = ["{}/v1", "/chat/completions"].concat();
        assert!(
            !source.contains(&old_format),
            "native provider must not hard-code a second /v1 segment"
        );
        assert!(
            source.contains("/chat/completions"),
            "native provider must compose /chat/completions from the API prefix"
        );
    }

    #[test]
    fn remaining_constructors_share_one_api_prefix_contract() {
        let _ = Arc::new(OpenAiConfig::openai("key".to_string()));
        let with_key = OpenAiConfig::default_with_key("key".to_string());
        assert_eq!(with_key.base_url, "https://api.openai.com/v1");
        let explicit = OpenAiConfig::openai("key".to_string());
        assert_eq!(explicit.base_url, "https://api.openai.com/v1");
    }
}
