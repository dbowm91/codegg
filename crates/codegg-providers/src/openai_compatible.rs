use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::{
    create_http_client, ChatRequest, ContentPart, EventStream, Message, ModelInfo, Provider,
    ReasoningVisibility,
};
use async_trait::async_trait;
use http::header::{HeaderName, HeaderValue};
use serde_json::json;

use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Specific(String),
}

#[derive(Clone)]
pub struct OpenAiCompatibleConfig {
    pub credential: Credential,
    pub base_url: String,
    pub auth_header: String,
    pub extra_headers: Vec<(String, String)>,
    pub models: Vec<ModelInfo>,
    pub tool_choice: ToolChoice,
}

/// A resolved model-discovery call target.
struct ModelsEndpoint {
    method: http::Method,
    url: String,
    /// Whether the reviewed profile marks discovery as a precondition for
    /// this provider being usable.
    required: bool,
}

/// Percent-encode a profile-declared query map into a `?a=b&c=d` suffix.
///
/// Returns an empty string when there is nothing to encode, so the common
/// no-query case does not leave a bare `?` on the URL.
pub(crate) fn encode_query(query: &std::collections::BTreeMap<String, String>) -> String {
    if query.is_empty() {
        return String::new();
    }
    let pairs = query
        .iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>();
    format!("?{}", pairs.join("&"))
}

/// Encode one query key or value using the conservative RFC 3986 unreserved
/// set, leaving nothing ambiguous about how the upstream parses the result.
fn encode_component(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Outcome for a discovery attempt that produced nothing.
///
/// There is no shipped catalog to fall back to. Config-declared seeds are
/// returned whenever the operator supplied any, so a provider with no working
/// discovery is still drivable from configuration. When the reviewed profile
/// marks discovery as required *and* there is nothing else to report, the
/// truthful answer is a failure: silently reporting an empty catalog would let
/// a provider look ready when it is not.
pub(crate) fn discovery_failed(
    provider_id: &str,
    required: bool,
    seeds: Vec<ModelInfo>,
) -> Result<Vec<ModelInfo>, ProviderError> {
    if required && seeds.is_empty() {
        return Err(ProviderError::api(
            "discovery_required",
            format!("{provider_id} declares model discovery required but none succeeded"),
        ));
    }
    Ok(seeds)
}

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    pub id: String,
    pub name: String,
    pub config: OpenAiCompatibleConfig,
    session_affinity_header: Option<HeaderName>,
    client: eggfetch_core::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(id: &str, name: &str, config: OpenAiCompatibleConfig) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            config,
            session_affinity_header: None,
            client: create_http_client(),
        }
    }

    pub fn simple(id: &str, name: &str, api_key: &str, base_url: &str) -> Self {
        Self::simple_with_credential(id, name, Credential::api_key(api_key), base_url)
    }

    /// Construct a provider that accepts a full [`Credential`] envelope.
    ///
    /// This preserves the [`crate::auth::CredentialKind`] (and any future
    /// metadata such as `expires_at`) so the registered provider can be
    /// used as-is by code that wants to inspect the credential type, not
    /// just the secret.
    pub fn simple_with_credential(
        id: &str,
        name: &str,
        credential: Credential,
        base_url: &str,
    ) -> Self {
        Self::new(
            id,
            name,
            OpenAiCompatibleConfig {
                credential,
                base_url: base_url.to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: Vec::new(),
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        )
    }

    /// Require one request-context session identity under a provider-owned
    /// transport header. The name is configuration, never caller input.
    pub fn with_session_affinity_header(mut self, name: &str) -> Result<Self, ProviderError> {
        self.session_affinity_header =
            Some(HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::api("invalid_header", "session-affinity header name is invalid")
            })?);
        Ok(self)
    }

    fn request_builder(
        &self,
        request: &ChatRequest,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<eggfetch_core::RequestBuilder, ProviderError> {
        let auth_name =
            HeaderName::from_bytes(self.config.auth_header.as_bytes()).map_err(|_| {
                ProviderError::api(
                    "invalid_header",
                    "configured authentication header is invalid",
                )
            })?;
        let auth_value = HeaderValue::from_str(
            &self.config.credential.authorization_header_value(),
        )
        .map_err(|_| {
            ProviderError::api(
                "invalid_header",
                "configured authentication value is invalid",
            )
        })?;

        let mut reserved = vec![auth_name.clone(), HeaderName::from_static("content-type")];
        if let Some(name) = &self.session_affinity_header {
            reserved.push(name.clone());
        }

        let auth_value = auth_value.to_str().map_err(|_| {
            ProviderError::api(
                "invalid_header",
                "configured authentication value is invalid",
            )
        })?;
        let mut builder = self
            .client
            .post(url)
            .map_err(ProviderError::from)?
            .header(auth_name.as_str(), auth_value)
            .header("content-type", "application/json");

        let mut extra_names = Vec::with_capacity(self.config.extra_headers.len());
        for (name, value) in &self.config.extra_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::api("invalid_header", "configured extra header name is invalid")
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                ProviderError::api("invalid_header", "configured extra header value is invalid")
            })?;
            if reserved.contains(&header_name) || extra_names.contains(&header_name) {
                return Err(ProviderError::api(
                    "reserved_header_collision",
                    "configured extra header collides with a transport-owned header",
                ));
            }
            extra_names.push(header_name.clone());
            let header_value = header_value.to_str().map_err(|_| {
                ProviderError::api("invalid_header", "configured extra header value is invalid")
            })?;
            builder = builder.header(header_name.as_str(), header_value);
        }

        if let Some(session_header) = &self.session_affinity_header {
            let session_id = request.context.session_id.as_deref().ok_or_else(|| {
                ProviderError::api(
                    "missing_session_context",
                    "provider requires a canonical session context",
                )
            })?;
            let session_value = HeaderValue::from_str(session_id).map_err(|_| {
                ProviderError::api(
                    "invalid_session_context",
                    "session context is not a valid HTTP header value",
                )
            })?;
            let session_value = session_value.to_str().map_err(|_| {
                ProviderError::api(
                    "invalid_session_context",
                    "session context is not a valid HTTP header value",
                )
            })?;
            builder = builder.header(session_header.as_str(), session_value);
        }

        builder
            .json(body)
            .map_err(|e| ProviderError::api("serialization", e.to_string()))
    }

    pub fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        self.try_build_body(request).unwrap_or_else(|error| {
            tracing::error!("shared OpenAI request encoding failed: {}", error);
            serde_json::Value::Null
        })
    }

    fn try_build_body(&self, request: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        let adapter = request
            .context
            .wire_policy
            .as_deref()
            .cloned()
            .unwrap_or_default();
        let mut body =
            crate::wire::encode_openai_chat(request, Some(&self.config.tool_choice), false)?;
        // The generic compatible contract intentionally did not forward
        // generation controls; preserve that per M001's provider matrix.
        crate::wire::omit_openai_fields(
            &mut body,
            &[
                "temperature",
                "top_p",
                "max_tokens",
                "response_format",
                "reasoning_effort",
            ],
        );
        if request.tools.is_none() {
            body["tools"] = serde_json::Value::Null;
        }
        let has_tools = request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty());
        if !has_tools {
            body.as_object_mut()
                .map(|object| object.remove("tool_choice"));
        }
        if let Some((field, configured_value)) = thinking_transform(&adapter) {
            let value = if configured_value.as_deref() == Some("true") {
                json!(request.thinking_budget != Some(0))
            } else if configured_value.as_deref() == Some("false") {
                json!(false)
            } else {
                json!(configured_value.unwrap_or_else(|| "true".to_string()))
            };
            body["chat_template_kwargs"] = json!({field: value});
        }
        if adapter.include_reasoning_content {
            let projected_messages = crate::project_tool_call_history(&request.messages);
            let reasoning = projected_messages
                .iter()
                .filter_map(|message| match message {
                    Message::Assistant { content, .. } => {
                        Some(content.iter().find_map(|part| match part {
                            ContentPart::Reasoning {
                                text,
                                visibility: ReasoningVisibility::Private,
                            } => Some(text.as_str()),
                            _ => None,
                        }))
                    }
                    _ => None,
                });
            if let Some(messages) = body["messages"].as_array_mut() {
                for (message, reasoning) in messages
                    .iter_mut()
                    .filter(|message| message["role"] == "assistant")
                    .zip(reasoning)
                {
                    if let Some(reasoning) = reasoning {
                        message["reasoning_content"] = json!(reasoning);
                    }
                }
            }
        }
        apply_policy_aliases(&mut body, &adapter);
        Ok(body)
    }

    /// The discovery endpoint to call, and whether the profile requires it.
    ///
    /// Method, path, and query come from the shared provider profile when this
    /// provider has a reviewed entry there, so a provider's discovery contract
    /// is data CodeGG consumes rather than a CodeGG-local URL constant.
    ///
    /// A provider with no shared-profile entry — an operator-configured
    /// gateway, for instance — uses the conventional OpenAI-compatible
    /// `GET {base_url}/models` against the *operator's own* base URL. That is
    /// operator configuration plus the widely-implemented convention, not a
    /// compiled-in model catalog. See
    /// [`crate::provider_profile::resolved_models_endpoint`].
    fn discovery_endpoint(&self) -> Result<ModelsEndpoint, ProviderError> {
        let base = self.config.base_url.trim_end_matches('/');
        match crate::provider_profile::resolved_models_endpoint(&self.id) {
            Ok(endpoint) => {
                // A profile that declares a request body cannot be honoured
                // without a TOML value in hand. No bundled profile declares
                // one; if one ever does, fail the call rather than silently
                // sending a request that is missing its declared body.
                if endpoint.body.is_some() {
                    return Err(ProviderError::api(
                        "provider_profile_contract",
                        "discovery endpoint declares a request body this transport cannot encode",
                    ));
                }
                let method =
                    http::Method::from_bytes(endpoint.method.as_bytes()).map_err(|_| {
                        ProviderError::api(
                            "provider_profile_contract",
                            "discovery endpoint declares an invalid HTTP method",
                        )
                    })?;
                Ok(ModelsEndpoint {
                    method,
                    url: format!("{base}{}{}", endpoint.path, encode_query(&endpoint.query)),
                    required: endpoint.required,
                })
            }
            Err(crate::provider_profile::ProfileError::ProviderNotAdapted { .. }) => {
                Ok(ModelsEndpoint {
                    method: http::Method::GET,
                    url: format!("{base}/models"),
                    required: false,
                })
            }
            // A profile that exists but could not be read is a broken contract,
            // not a licence to guess a local endpoint.
            Err(error) => Err(ProviderError::api(
                "provider_profile_contract",
                format!("shared provider profile could not resolve discovery: {error}"),
            )),
        }
    }
}

fn thinking_transform(adapter: &crate::ProviderWirePolicy) -> Option<(&str, Option<String>)> {
    adapter
        .enable_thinking
        .map(|enabled| ("enable_thinking", Some(enabled.to_string())))
}

fn wire_tool_name(adapter: &crate::ProviderWirePolicy, name: &str) -> String {
    adapter
        .tool_aliases
        .get(name)
        .map(String::as_str)
        .unwrap_or(name)
        .to_string()
}

fn alias_parameter_properties(
    adapter: &crate::ProviderWirePolicy,
    tool_name: &str,
    parameters: &mut serde_json::Value,
) {
    let wire_name = wire_tool_name(adapter, tool_name);
    let Some(properties) = parameters
        .get_mut("properties")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    if let Some(aliases) = adapter.argument_aliases.get(&wire_name) {
        for (canonical, wire) in aliases {
            if let Some(schema) = properties.remove(canonical) {
                properties.insert(wire.clone(), schema);
            }
        }
    }
}

fn apply_policy_aliases(body: &mut serde_json::Value, adapter: &crate::ProviderWirePolicy) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    if let Some(tools) = object
        .get_mut("tools")
        .and_then(serde_json::Value::as_array_mut)
    {
        for tool in tools {
            let Some(function) = tool.get_mut("function") else {
                continue;
            };
            if let Some(name) = function
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
            {
                function["name"] = json!(wire_tool_name(adapter, &name));
                if let Some(parameters) = function.get_mut("parameters") {
                    alias_parameter_properties(adapter, &name, parameters);
                }
            }
        }
    }
    if let Some(messages) = object
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
    {
        for message in messages {
            if let Some(calls) = message
                .get_mut("tool_calls")
                .and_then(serde_json::Value::as_array_mut)
            {
                for call in calls {
                    if let Some(function) = call.get_mut("function") {
                        if let Some(name) = function
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                        {
                            function["name"] = json!(wire_tool_name(adapter, &name));
                        }
                    }
                }
            }
        }
    }
    if let Some(choice) = object.get_mut("tool_choice") {
        if let Some(name) = choice
            .pointer("/function/name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        {
            choice["function"]["name"] = json!(wire_tool_name(adapter, &name));
        }
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = self.try_build_body(request)?;

        if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
            let body_str = serde_json::to_string_pretty(&body).unwrap_or_default();
            let preview: String = body_str.chars().take(4000).collect();
            tracing::info!(
                "openai_compatible request body: url={}, model={}, body_len={}, body_preview={}",
                url,
                request.model,
                body_str.len(),
                preview
            );
        }

        let tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let tool_preview = request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .take(4)
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "none".to_string());
        let first_tool_arg_shape = body
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|msgs| {
                msgs.iter().find_map(|msg| {
                    msg.get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|tc| tc.get("function"))
                        .and_then(|f| f.get("arguments"))
                        .map(|arg| {
                            if arg.is_string() {
                                "string"
                            } else if arg.is_object() {
                                "object"
                            } else if arg.is_array() {
                                "array"
                            } else if arg.is_null() {
                                "null"
                            } else if arg.is_number() {
                                "number"
                            } else if arg.is_boolean() {
                                "boolean"
                            } else {
                                "unknown"
                            }
                        })
                })
            })
            .unwrap_or("none");
        debug_log!(
            "openai_compatible request debug: model='{}', tool_count={}, tool_preview='{}', first_tool_arg_shape={}",
            request.model,
            tool_count,
            tool_preview,
            first_tool_arg_shape
        );

        let mut resp = {
            tracing::debug!(
                "OpenAiCompatible({}): sending request to {}, auth_header={}, model={}",
                self.name,
                url,
                self.config.auth_header,
                request.model
            );
            self.request_builder(request, &url, &body)?
                .send()
                .await
                .map_err(ProviderError::from)?
        };

        if resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(resp.headers()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            tracing::error!(
                "OpenAiCompatible({}): API error ({}): {}",
                self.name,
                status,
                err
            );
            if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                let preview: String = err.chars().take(2000).collect();
                tracing::info!("openai_compatible error body: {}", preview);
            }
            // Preserve the numeric status so the retry taxonomy can
            // distinguish permanent auth/invalid-request failures from
            // transient 5xx without reparsing the message body.
            return Err(ProviderError::from_http_status(
                status.as_u16(),
                format!("API error: {err}"),
            ));
        }

        let stream = resp.bytes_stream().map_err(ProviderError::from)?;
        let wire_policy = Some(
            request
                .context
                .wire_policy
                .clone()
                .unwrap_or_else(|| std::sync::Arc::new(crate::ProviderWirePolicy::default())),
        );
        Ok(crate::wire::openai_chat_stream(
            stream,
            wire_policy,
            Some(Duration::from_secs(30)),
        ))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // Config-declared models are additive seeds. Discovery is always
        // attempted and its results are unioned on top of them; the seeds never
        // suppress the attempt and never replace what discovery returns.
        let mut models = self.config.models.clone();
        let endpoint = self.discovery_endpoint()?;

        // Shared bounded discovery core limits (same response-byte,
        // model-count, and string-length bounds as the strict provisioning
        // probe).
        let options = crate::eggpool::EggpoolProbeOptions::default();

        let request = self
            .client
            .request(endpoint.method.clone(), &endpoint.url)
            .map_err(ProviderError::from)?
            .timeout(crate::provider_core::non_streaming_timeout())
            .max_decoded_body_size(options.response_byte_limit)
            .header(
                &self.config.auth_header,
                &self.config.credential.authorization_header_value(),
            );

        let mut response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("discovery failed for {}: {}", self.name, error);
                return discovery_failed(&self.id, endpoint.required, models);
            }
        };

        if !response.status().is_success() {
            tracing::warn!(
                "discovery for {} returned HTTP {}",
                self.name,
                response.status()
            );
            return discovery_failed(&self.id, endpoint.required, models);
        }

        if response
            .content_length()
            .is_some_and(|length| length > options.response_byte_limit as u64)
        {
            return discovery_failed(&self.id, endpoint.required, models);
        }

        let body = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return discovery_failed(&self.id, endpoint.required, models),
        };

        let discovered = match crate::eggpool::parse_compatible_models_response(&body, &options) {
            Ok(summaries) => summaries,
            Err(_) => return discovery_failed(&self.id, endpoint.required, models),
        };

        for summary in discovered {
            if !models.iter().any(|m| m.id == summary.id) {
                models.push(ModelInfo {
                    id: summary.id.clone(),
                    name: summary.name,
                    provider: self.id.clone(),
                    // A `/models` response advertises an id and a display name,
                    // not capabilities. Every field below therefore means
                    // "not advertised by discovery" rather than a capability
                    // CodeGG invented. `ModelInfo` types these as `bool`/`usize`
                    // rather than `Option`, so "unknown" cannot be expressed
                    // directly; `false` and `0` are the honest encoding and match
                    // the existing unknown-model placeholder used by session
                    // selection. Nothing in the runtime gates tool definitions on
                    // `supports_tools`, so these fields are display and
                    // persistence metadata only.
                    context_window: 0,
                    max_output_tokens: None,
                    supports_tools: false,
                    supports_vision: false,
                    variants: Vec::new(),
                });
            }
        }

        Ok(models)
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_types::CredentialKind;
    use crate::{ContentPart, Message, ProviderRequestContext};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Arc};
    use std::thread;

    struct CapturedRequest {
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
        (format!("http://{address}/v1"), rx, handle)
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
            headers,
            body: String::from_utf8_lossy(&raw[header_end..header_end + content_length]).into(),
        }
    }

    fn request(session_id: Option<&str>) -> ChatRequest {
        ChatRequest {
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

    fn header_values<'a>(request: &'a CapturedRequest, name: &str) -> Vec<&'a str> {
        request
            .headers
            .iter()
            .filter(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    #[test]
    fn simple_with_credential_preserves_bearer_kind() {
        let cred = Credential::bearer("short-lived-token", None);
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "openai",
            "OpenAI",
            cred.clone(),
            "https://api.example.test/v1",
        );
        assert_eq!(provider.config.credential.kind, CredentialKind::BearerToken);
        assert_eq!(
            provider.config.credential.authorization_header_value(),
            "Bearer short-lived-token"
        );
    }

    #[test]
    fn simple_with_credential_preserves_api_key_kind() {
        let cred = Credential::api_key("sk-test-1234");
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "openai",
            "OpenAI",
            cred,
            "https://api.example.test/v1",
        );
        assert_eq!(provider.config.credential.kind, CredentialKind::ApiKey);
        assert_eq!(
            provider.config.credential.authorization_header_value(),
            "Bearer sk-test-1234"
        );
    }

    #[test]
    fn simple_wraps_api_key() {
        // Backwards-compat: `simple` should build a Credential::api_key under
        // the hood so existing callers see the same behavior.
        let provider =
            OpenAiCompatibleProvider::simple("xai", "xAI", "sk-x", "https://api.x.ai/v1");
        assert_eq!(provider.config.credential.kind, CredentialKind::ApiKey);
        assert_eq!(provider.config.credential.secret, "sk-x");
    }

    #[tokio::test]
    async fn stored_bearer_reaches_transport_with_bearer_header() {
        // M010: representative full-credential path (xai) preserves kind
        // end-to-end. Uses a synthetic sentinel secret; the test inspects
        // the wire header and never logs the secret.
        use crate::Provider as _;
        let (base_url, rx, server) = spawn_capture_server(1);
        let credential = Credential::bearer("m010-sentinel-bearer", None);
        assert_eq!(credential.kind, CredentialKind::BearerToken);
        let provider =
            OpenAiCompatibleProvider::simple_with_credential("xai", "xAI", credential, &base_url);
        let _stream = provider.stream(&request(None)).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured, "authorization"),
            vec!["Bearer m010-sentinel-bearer"]
        );
        assert!(!captured.body.contains("m010-sentinel-bearer"));
    }

    #[tokio::test]
    async fn stored_api_key_reaches_transport_with_bearer_header() {
        // Compatibility: existing API-key behavior is unchanged on the same
        // full-credential transport.
        use crate::Provider as _;
        let (base_url, rx, server) = spawn_capture_server(1);
        let credential = Credential::api_key("m010-sentinel-apikey");
        let provider =
            OpenAiCompatibleProvider::simple_with_credential("xai", "xAI", credential, &base_url);
        let _stream = provider.stream(&request(None)).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured, "authorization"),
            vec!["Bearer m010-sentinel-apikey"]
        );
    }

    #[tokio::test]
    async fn session_affinity_is_stable_isolated_and_not_in_body() {
        let (base_url, rx, server) = spawn_capture_server(3);
        let provider =
            OpenAiCompatibleProvider::simple("opencode_go", "OpenCode Go", "test-key", &base_url)
                .with_session_affinity_header("x-opencode-session")
                .expect("test session header name is valid");

        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request 1");
        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request 2");
        let _stream = provider
            .stream(&request(Some("S2")))
            .await
            .expect("request 3");

        let captured: Vec<_> = (0..3)
            .map(|_| rx.recv().expect("captured request"))
            .collect();
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured[0], "x-opencode-session"),
            vec!["S1"]
        );
        assert_eq!(
            header_values(&captured[1], "x-opencode-session"),
            vec!["S1"]
        );
        assert_eq!(
            header_values(&captured[2], "x-opencode-session"),
            vec!["S2"]
        );
        assert!(!captured
            .iter()
            .any(|request| request.body.contains("S1") || request.body.contains("S2")));
    }

    #[tokio::test]
    async fn required_session_context_fails_before_network_io() {
        let (base_url, rx, server) = spawn_capture_server(0);
        let provider =
            OpenAiCompatibleProvider::simple("opencode_go", "OpenCode Go", "test-key", &base_url)
                .with_session_affinity_header("x-opencode-session")
                .expect("test session header name is valid");

        let error = match provider.stream(&request(None)).await {
            Ok(_) => panic!("missing context must fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::Api { ref code, .. } if code == "missing_session_context")
        );
        assert!(
            rx.try_recv().is_err(),
            "missing context sent a network request"
        );
        server.join().expect("capture server");
    }

    #[tokio::test]
    async fn session_context_is_not_global_header_leak() {
        let (base_url, rx, server) = spawn_capture_server(1);
        let provider = OpenAiCompatibleProvider::simple("openai", "OpenAI", "test-key", &base_url);
        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert!(header_values(&captured, "x-opencode-session").is_empty());
    }

    #[tokio::test]
    async fn extra_headers_are_sent_and_reserved_collisions_fail_locally() {
        let (base_url, rx, server) = spawn_capture_server(1);
        let provider = OpenAiCompatibleProvider::new(
            "copilot",
            "Copilot",
            OpenAiCompatibleConfig {
                credential: Credential::api_key("test-key"),
                base_url,
                auth_header: "Authorization".to_string(),
                extra_headers: vec![("Editor-Version".to_string(), "codegg/test".to_string())],
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        );
        let _stream = provider.stream(&request(None)).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured, "editor-version"),
            vec!["codegg/test"]
        );

        for extra_headers in [
            vec![("authorization".to_string(), "override".to_string())],
            vec![("CONTENT-TYPE".to_string(), "text/plain".to_string())],
            vec![("bad\r\nname".to_string(), "value".to_string())],
            vec![("X-Test".to_string(), "bad\r\nvalue".to_string())],
        ] {
            let provider = OpenAiCompatibleProvider::new(
                "openai",
                "OpenAI",
                OpenAiCompatibleConfig {
                    credential: Credential::api_key("test-key"),
                    base_url: "http://127.0.0.1:1/v1".to_string(),
                    auth_header: "Authorization".to_string(),
                    extra_headers,
                    models: Vec::new(),
                    tool_choice: ToolChoice::Auto,
                },
            );
            let error = match provider.stream(&request(None)).await {
                Ok(_) => panic!("invalid extra header must fail"),
                Err(error) => error,
            };
            assert!(
                matches!(error, ProviderError::Api { ref code, .. } if code == "reserved_header_collision" || code == "invalid_header")
            );
        }

        let provider = OpenAiCompatibleProvider::new(
            "opencode_go",
            "OpenCode Go",
            OpenAiCompatibleConfig {
                credential: Credential::api_key("test-key"),
                base_url: "http://127.0.0.1:1/v1".to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: vec![("X-OPENCODE-SESSION".to_string(), "other".to_string())],
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        )
        .with_session_affinity_header("x-opencode-session")
        .expect("test session header name is valid");
        let error = match provider.stream(&request(Some("S1"))).await {
            Ok(_) => panic!("session header collision must fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::Api { ref code, .. } if code == "reserved_header_collision")
        );
    }

    fn spawn_models_server(status: u16, body: String) -> (String, thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind models server");
        let addr = listener.local_addr().expect("models server address");
        let url = format!("http://{addr}");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept models request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("set models timeout");
            let mut raw = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let count = stream.read(&mut chunk).expect("read models request");
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&chunk[..count]);
                if raw.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let reason = if status == 200 { "OK" } else { "ERROR" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            use std::io::Write;
            stream
                .write_all(response.as_bytes())
                .expect("write models response");
        });
        (url, handle)
    }

    fn seed_model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            provider: "test".to_string(),
            context_window: 128_000,
            max_output_tokens: None,
            supports_tools: true,
            supports_vision: false,
            variants: Vec::new(),
        }
    }

    fn models_provider(base_url: &str, seeds: Vec<ModelInfo>) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            "test",
            "Test",
            OpenAiCompatibleConfig {
                credential: Credential::api_key("test-key"),
                base_url: base_url.to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: Vec::new(),
                models: seeds,
                tool_choice: ToolChoice::Auto,
            },
        )
    }

    #[tokio::test]
    async fn best_effort_models_merges_live_ids_and_keeps_seeds_on_failure() {
        use crate::Provider as _;
        // Success merges live IDs without duplicating seeds.
        let (url, server) = spawn_models_server(
            200,
            r#"{"data":[{"id":"live-a"},{"id":"seed-keep","name":"Seed Keep"}]}"#.to_string(),
        );
        let provider =
            models_provider(&url, vec![seed_model("seed-keep"), seed_model("seed-only")]);
        let models = provider.models().await.expect("best-effort models");
        server.join().expect("models server joins");
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"seed-keep"));
        assert!(ids.contains(&"seed-only"));
        assert!(ids.contains(&"live-a"));
        assert_eq!(ids.len(), 3);

        // Invalid JSON falls back to seeds unchanged.
        let (url, server) = spawn_models_server(200, "not-json".to_string());
        let provider = models_provider(&url, vec![seed_model("seed-only")]);
        let models = provider.models().await.expect("fallback on invalid JSON");
        server.join().expect("models server joins");
        assert_eq!(vec!["seed-only"], ids_of(&models));

        // Oversized count falls back to seeds (bounded core).
        let many: Vec<String> = (0..300).map(|i| format!(r#"{{"id":"m{i}"}}"#)).collect();
        let oversized = format!(r#"{{"data":[{}]}}"#, many.join(","));
        let (url, server) = spawn_models_server(200, oversized);
        let provider = models_provider(&url, vec![seed_model("seed-only")]);
        let models = provider.models().await.expect("fallback on oversized");
        server.join().expect("models server joins");
        assert_eq!(vec!["seed-only"], ids_of(&models));

        // Non-success status falls back to seeds.
        let (url, server) = spawn_models_server(500, r#"{"error":"boom"}"#.to_string());
        let provider = models_provider(&url, vec![seed_model("seed-only")]);
        let models = provider.models().await.expect("fallback on status");
        server.join().expect("models server joins");
        assert_eq!(vec!["seed-only"], ids_of(&models));
    }

    fn ids_of(models: &[ModelInfo]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }
}
