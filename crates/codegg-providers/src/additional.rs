use crate::anthropic::AnthropicProvider;
use crate::auth_types::Credential;
use crate::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider, ToolChoice};
use crate::setup_catalog::{
    CEREBRAS_BASE_URL, COHERE_BASE_URL, DEEPINFRA_BASE_URL, GENERALCOMPUTE_BASE_URL, GROQ_BASE_URL,
    MINIMAX_BASE_URL, MISTRAL_BASE_URL, OPENCODE_GO_BASE_URL, PERPLEXITY_BASE_URL,
    TOGETHER_BASE_URL, VENICE_BASE_URL, XAI_BASE_URL,
};
use crate::{ModelInfo, Provider};

/// xAI models exposed by the OpenAI-compatible endpoint.
fn xai_models() -> Vec<ModelInfo> {
    vec![ModelInfo {
        id: "grok-build-0.1".to_string(),
        name: "Grok Build 0.1".to_string(),
        provider: "xai".to_string(),
        context_window: 256_000,
        max_output_tokens: None,
        supports_tools: true,
        supports_vision: false,
        variants: vec![],
    }]
}

pub fn create_xai(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::new(
        "xai",
        "xAI",
        OpenAiCompatibleConfig {
            credential,
            base_url: XAI_BASE_URL.to_string(),
            auth_header: "Authorization".to_string(),
            extra_headers: Vec::new(),
            models: xai_models(),
            tool_choice: ToolChoice::Auto,
        },
    )
}

pub fn create_mistral(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "mistral",
        "Mistral",
        credential,
        MISTRAL_BASE_URL,
    )
}

pub fn create_groq(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential("groq", "Groq", credential, GROQ_BASE_URL)
}

pub fn create_deepinfra(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "deepinfra",
        "DeepInfra",
        credential,
        DEEPINFRA_BASE_URL,
    )
}

pub fn create_cerebras(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cerebras",
        "Cerebras",
        credential,
        CEREBRAS_BASE_URL,
    )
}

pub fn create_cohere(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cohere",
        "Cohere",
        credential,
        COHERE_BASE_URL,
    )
}

pub fn create_together(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "together",
        "Together AI",
        credential,
        TOGETHER_BASE_URL,
    )
}

pub fn create_perplexity(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "perplexity",
        "Perplexity",
        credential,
        PERPLEXITY_BASE_URL,
    )
}

pub fn create_venice(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "venice",
        "Venice",
        credential,
        VENICE_BASE_URL,
    )
}

pub fn create_generalcompute(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "generalcompute",
        "GeneralCompute",
        credential,
        GENERALCOMPUTE_BASE_URL,
    )
}

pub fn create_minimax(api_key: String) -> impl Provider {
    debug_log!(
        "create_minimax: using Anthropic-compatible endpoint at https://api.minimax.io/anthropic"
    );
    let models = vec![
        ModelInfo {
            id: "minimax/minimax-2.7".to_string(),
            name: "minimax/minimax-2.7".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.7-highspeed".to_string(),
            name: "minimax/minimax-2.7-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.5".to_string(),
            name: "minimax/minimax-2.5".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.5-highspeed".to_string(),
            name: "minimax/minimax-2.5-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.1".to_string(),
            name: "minimax/minimax-2.1".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.1-highspeed".to_string(),
            name: "minimax/minimax-2.1-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
    ];
    AnthropicProvider::new(api_key)
        .with_base_url(MINIMAX_BASE_URL.to_string())
        .with_id("minimax".to_string())
        .with_name("MiniMax".to_string())
        .with_models(models)
}

pub fn create_sap_ai_core(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("sap_ai_core", "SAP AI Core", &api_key, &base_url)
}

pub fn create_zenmux(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("zenmux", "Zenmux", &api_key, &base_url)
}

pub fn create_kilo(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("kilo", "Kilo", &api_key, &base_url)
}

pub fn create_vercel_ai_gateway(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "vercel_ai_gateway",
        "Vercel AI Gateway",
        &api_key,
        &base_url,
    )
}

pub fn create_opencode_go(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "opencode_go",
        "OpenCode Go",
        credential,
        OPENCODE_GO_BASE_URL,
    )
    .with_session_affinity_header("x-opencode-session")
    .expect("built-in OpenCode Go session header name is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentPart, Message, ProviderRequestContext};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    fn chat_request(session_id: Option<&str>) -> crate::ChatRequest {
        crate::ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            }],
            model: "test-model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: ProviderRequestContext {
                session_id: session_id.map(Arc::from),
                ..Default::default()
            },
        }
    }

    fn read_request_line(stream: &mut TcpStream) -> (String, Vec<(String, String)>) {
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
        // Drain body so the client can complete.
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
            let count = stream.read(&mut chunk).expect("read body");
            assert!(count > 0, "provider closed before body");
            raw.extend_from_slice(&chunk[..count]);
        }
        let mut headers = Vec::new();
        for line in header_text.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        (request_line, headers)
    }

    #[test]
    fn opencode_go_uses_current_zen_prefix() {
        assert_eq!(OPENCODE_GO_BASE_URL, "https://opencode.ai/zen/go/v1");
    }

    #[tokio::test]
    async fn opencode_go_chat_uses_exact_path_with_session_affinity() {
        use crate::Provider as _;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let addr = listener.local_addr().expect("capture address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept chat request");
            let (request_line, headers) = read_request_line(&mut stream);
            let session = headers
                .iter()
                .find(|(name, _)| name == "x-opencode-session")
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
            let response_body =
                b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            )
            .expect("write headers");
            stream.write_all(response_body).expect("write body");
            (request_line, session)
        });
        let base_url = format!("http://{addr}/zen/go/v1");
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "opencode_go",
            "OpenCode Go",
            Credential::api_key("test-key"),
            &base_url,
        )
        .with_session_affinity_header("x-opencode-session")
        .expect("session header");
        let _stream = provider
            .stream(&chat_request(Some("S1")))
            .await
            .expect("chat request");
        let (request_line, session) = handle.join().expect("capture joins");
        assert!(
            request_line.contains("POST /zen/go/v1/chat/completions "),
            "unexpected chat path: {request_line}"
        );
        assert_eq!(session, "S1");
    }

    #[tokio::test]
    async fn opencode_go_models_uses_exact_models_path() {
        use crate::Provider as _;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind models server");
        let addr = listener.local_addr().expect("models address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept models request");
            let (request_line, _) = read_request_line(&mut stream);
            let body = r#"{"data":[{"id":"glm-5.1"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write models response");
            request_line
        });
        let base_url = format!("http://{addr}/zen/go/v1");
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "opencode_go",
            "OpenCode Go",
            Credential::api_key("test-key"),
            &base_url,
        );
        let models = provider.models().await.expect("models");
        let request_line = handle.join().expect("models joins");
        assert!(
            request_line.contains("GET /zen/go/v1/models "),
            "unexpected models path: {request_line}"
        );
        assert!(models.iter().any(|m| m.id == "glm-5.1"));
    }
}
