//! OpenCode Go direct multi-surface provider.
//!
//! OpenCode Go serves one provider identity across three wire surfaces. Which
//! surface a given model uses is **not** discoverable from the public `/models`
//! response, which exposes ids but no wire field, so CodeGG resolves it from
//! the shared EggPool provider-profile contract instead of guessing.
//!
//! Ownership boundaries:
//!
//! * The shared profile owns secret-free metadata only: base URL, per-surface
//!   paths, per-surface auth *shape*, and exact model-to-wire hints.
//! * CodeGG owns credentials, secret references, HTTP transport, cancellation,
//!   deadlines, retry policy, and the typed error taxonomy.
//! * `eggpool-wire` owns request grammar and stream decoding.
//!
//! Exactly one surface is resolved before any network I/O. There is no
//! speculative retry from one surface to another: once a request could have
//! reached model execution, a failure is reported, not re-routed.

use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::openai_compatible::OpenAiCompatibleProvider;
use crate::provider_profile::{self, ProfileError, RouteAuth, SurfaceRoute};
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;
use eggpool_wire::profile::WireSurface;
use http::header::{HeaderName, HeaderValue};

use std::time::Duration;

/// Transport-owned deadline applied identically on every surface.
///
/// Surface-independent by construction so cancellation behavior cannot drift
/// between Chat, Responses, and Messages.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(30);

/// Provider-owned session-affinity header established by M008/M009.
const SESSION_HEADER: &str = "x-opencode-session";

/// Stable id of the single OpenCode Go connection identity.
pub const PROVIDER_ID: &str = "opencode_go";

/// Direct multi-surface provider for the OpenCode Go gateway.
#[derive(Clone)]
pub struct OpenCodeGoProvider {
    id: String,
    name: String,
    credential: Credential,
    session_header: HeaderName,
    client: eggfetch_core::Client,
    /// Shared bounded discovery machinery, reused from the compatible provider.
    ///
    /// `None` only when the shared profile has no entry for this provider. That
    /// is a broken-contract state, not a fallback condition: discovery fails
    /// closed rather than reaching for a CodeGG-local endpoint constant.
    discovery: Option<OpenAiCompatibleProvider>,
    /// Test-only origin redirection; see [`Self::with_capture_base`].
    #[cfg(any(test, feature = "capture-test-support"))]
    capture_base: Option<String>,
}

// C001: the capture seam this file exposes.
//
// The shared profile fixes the OpenCode Go origin, so a cross-layer trajectory
// test that resolves a *durable* connection through the real factory cannot
// observe the request on a loopback socket without redirecting the origin.
//
// The seam is `with_capture_base`, a per-instance builder consumed by
// `setup_catalog::build_opencode_go_with_capture_base` and exposed on
// `ProviderConnectionFactory`. It is deliberately constrained:
//
//   * compiled only under `cfg(test)` or the opt-in `capture-test-support`
//     feature, which no production target enables — the shipped binary links a
//     `codegg-providers` without it;
//   * never read from configuration, the protocol, an environment variable, or
//     any CLI path; there is no production setter;
//   * scoped to the constructed provider instance, so concurrent tests cannot
//     observe each other's origin;
//   * **origin only** — path, per-surface auth shape, and session header still
//     come from the shared profile, so a captured path is the production path.

impl OpenCodeGoProvider {
    /// Build the provider from the shared profile's base URL.
    ///
    /// Infallible by design because the built-in registration path cannot
    /// surface an error. A missing shared-profile entry degrades to a provider
    /// that fails every call with a contract error; it never silently reverts
    /// to a locally-owned endpoint.
    pub fn new(credential: Credential) -> Self {
        let id = PROVIDER_ID;
        let discovery = provider_profile::shared_base_url(id).map(|base_url| {
            OpenAiCompatibleProvider::simple_with_credential(
                id,
                "OpenCode Go",
                credential.clone(),
                &base_url,
            )
        });
        Self {
            id: id.to_string(),
            name: "OpenCode Go".to_string(),
            credential,
            session_header: HeaderName::from_static(SESSION_HEADER),
            client: create_http_client(),
            discovery,
            #[cfg(any(test, feature = "capture-test-support"))]
            capture_base: None,
        }
    }

    /// Redirect only the origin/base at a capture server.
    ///
    /// Test-only (`cfg(test)` or the opt-in `capture-test-support` feature) and
    /// unreachable from provider configuration, so the shared profile remains
    /// the single owner of endpoint facts. See the C001 note on this module.
    #[cfg(any(test, feature = "capture-test-support"))]
    pub fn with_capture_base(mut self, base: &str) -> Self {
        self.capture_base = Some(base.trim_end_matches('/').to_string());
        self
    }

    /// Resolve the single executable surface for this request, or fail locally.
    ///
    /// An unresolved model is a zero-network failure by construction: no
    /// request is built, no socket is opened, and nothing is defaulted to Chat.
    fn resolve(&self, request: &ChatRequest) -> Result<SurfaceRoute, ProviderError> {
        let route =
            provider_profile::resolve_route(&self.id, &request.model).map_err(profile_error)?;
        self.apply_capture_base(route)
    }

    /// No-op outside capture-capable test builds; see
    /// [`Self::with_capture_base`].
    #[cfg(not(any(test, feature = "capture-test-support")))]
    fn apply_capture_base(&self, route: SurfaceRoute) -> Result<SurfaceRoute, ProviderError> {
        Ok(route)
    }

    /// Redirect the origin while leaving the profile-owned path intact.
    #[cfg(any(test, feature = "capture-test-support"))]
    fn apply_capture_base(&self, mut route: SurfaceRoute) -> Result<SurfaceRoute, ProviderError> {
        if let Some(base) = &self.capture_base {
            let path = provider_profile::shared_base_url(&self.id)
                .and_then(|profile_base| route.url.strip_prefix(&profile_base).map(str::to_string))
                .unwrap_or_else(|| route.url.clone());
            route.url = format!("{base}{path}");
        }
        Ok(route)
    }

    /// Encode the semantic request with the shared kernel for the resolved surface.
    fn encode_body(
        &self,
        route: &SurfaceRoute,
        request: &ChatRequest,
    ) -> Result<serde_json::Value, ProviderError> {
        match route.surface {
            WireSurface::OpenaiChatCompletions => {
                crate::wire::encode_openai_chat(request, None, true)
            }
            WireSurface::OpenaiResponses => crate::wire::encode_openai_responses(request, true),
            WireSurface::AnthropicMessages => crate::wire::encode_anthropic_messages(request),
            // Gemini surfaces are not part of this provider's contract. Refusing
            // here keeps a mis-resolved hint a local error instead of a request
            // to an endpoint this provider does not own.
            other => Err(ProviderError::api(
                "wire_surface_unsupported",
                format!("wire surface {other:?} is not part of this provider's contract"),
            )),
        }
    }

    /// Apply the surface-selected credential shape.
    ///
    /// Per-surface auth is deterministic: Chat and Responses use Bearer,
    /// Messages uses `x-api-key`. A surface never receives the other's header,
    /// so a Messages request can never be sent with a Bearer credential or a
    /// Chat request with an `x-api-key` credential.
    fn apply_auth(&self, route: &SurfaceRoute) -> Result<(HeaderName, HeaderValue), ProviderError> {
        let (name, value) = match &route.auth {
            RouteAuth::Bearer { header, .. } => {
                (header.clone(), self.credential.authorization_header_value())
            }
            RouteAuth::ApiKeyHeader { header } => (header.clone(), self.credential.secret.clone()),
            RouteAuth::RawAuthorization { header } => {
                (header.clone(), self.credential.secret.clone())
            }
            RouteAuth::None => {
                return Err(ProviderError::api(
                    "unsupported_surface_auth",
                    "resolved surface declares no credential, which this provider cannot serve",
                ));
            }
        };
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            ProviderError::api("invalid_header", "profile auth header name is invalid")
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|_| {
            ProviderError::api(
                "invalid_header",
                "credential is not a valid HTTP header value",
            )
        })?;
        Ok((header_name, header_value))
    }

    /// Build the single HTTP request for the resolved surface.
    fn request_builder(
        &self,
        route: &SurfaceRoute,
        request: &ChatRequest,
        body: &serde_json::Value,
    ) -> Result<eggfetch_core::RequestBuilder, ProviderError> {
        let (auth_name, auth_value) = self.apply_auth(route)?;

        // Reserved headers are transport-owned. A profile-supplied static
        // header may never collide with the credential, session, or content
        // headers (M008 rule).
        let mut reserved = vec![auth_name.clone(), self.session_header.clone()];
        reserved.push(HeaderName::from_static("content-type"));

        // The stable session identity is required on every OpenCode Go surface
        // and is taken from request context, never from the profile.
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

        let mut builder = self
            .client
            .post(&route.url)
            .map_err(ProviderError::from)?
            .header(
                auth_name.as_str(),
                auth_value.to_str().map_err(|_| {
                    ProviderError::api(
                        "invalid_header",
                        "credential is not a valid HTTP header value",
                    )
                })?,
            )
            .header("content-type", "application/json")
            .header(
                self.session_header.as_str(),
                session_value.to_str().map_err(|_| {
                    ProviderError::api(
                        "invalid_session_context",
                        "session context is not a valid HTTP header value",
                    )
                })?,
            );

        let mut extra_names: Vec<HeaderName> = Vec::with_capacity(route.static_headers.len());
        for (name, value) in &route.static_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::api("invalid_header", "profile static header name is invalid")
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                ProviderError::api("invalid_header", "profile static header value is invalid")
            })?;
            if reserved.contains(&header_name) || extra_names.contains(&header_name) {
                return Err(ProviderError::api(
                    "reserved_header_collision",
                    "profile static header collides with a transport-owned header",
                ));
            }
            extra_names.push(header_name.clone());
            builder = builder.header(
                header_name.as_str(),
                header_value.to_str().map_err(|_| {
                    ProviderError::api("invalid_header", "profile static header value is invalid")
                })?,
            );
        }

        builder
            .json(body)
            .map_err(|error| ProviderError::api("serialization", error.to_string()))
    }
}

/// Map a profile-resolution failure to CodeGG's typed taxonomy.
///
/// These are all **local, pre-network** failures. They deliberately do not use
/// `ProviderError::Auth`: an unresolved wire mapping is a metadata problem, not
/// a credential problem, and mislabeling it would corrupt the M010 credential
/// qualification axis that real 401/403 inference feedback owns.
fn profile_error(error: ProfileError) -> ProviderError {
    match &error {
        // A model the profile cannot map is not executable here.
        ProfileError::WireUnresolved { .. } | ProfileError::SurfaceUnavailable { .. } => {
            ProviderError::ModelNotFound(error.to_string())
        }
        // Provider-level contract gaps are configuration failures.
        ProfileError::ProviderNotAdapted { .. }
        | ProfileError::ProviderUnknown { .. }
        | ProfileError::Contract(_) => ProviderError::api(
            "provider_profile_contract",
            "shared provider profile could not resolve this provider",
        ),
    }
}

/// Stream adapter matching the resolved surface.
fn stream_adapter(
    surface: WireSurface,
) -> Result<eggpool_wire::codec::StreamAdapterKind, ProviderError> {
    use eggpool_wire::codec::StreamAdapterKind;
    Ok(match surface {
        WireSurface::OpenaiChatCompletions => StreamAdapterKind::OpenaiChatSse,
        WireSurface::OpenaiResponses => StreamAdapterKind::OpenaiResponsesSse,
        WireSurface::AnthropicMessages => StreamAdapterKind::AnthropicMessagesSse,
        other => {
            return Err(ProviderError::api(
                "wire_surface_unsupported",
                format!("wire surface {other:?} is not part of this provider's contract"),
            ));
        }
    })
}

#[async_trait]
impl Provider for OpenCodeGoProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        // 1. Resolve model -> surface -> path/auth. Fails closed, zero network.
        let route = self.resolve(request)?;

        // 2/5. Encode through the shared kernel for the resolved surface.
        let body = self.encode_body(&route, request)?;

        // 6. One request. No retry to another surface is ever attempted.
        let builder = self.request_builder(&route, request, &body)?;
        let mut response = builder.send().await.map_err(ProviderError::from)?;

        if response.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(response.headers()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            // Preserve the numeric status so the typed taxonomy — and therefore
            // M010's credential axis — sees real 401/403 as auth failures.
            return Err(ProviderError::from_http_status(
                status.as_u16(),
                format!("API error: {body_text}"),
            ));
        }

        // 7. Decode the selected surface's stream back into CodeGG events.
        let adapter = stream_adapter(route.surface)?;
        let policy = request
            .context
            .wire_policy
            .clone()
            .or_else(|| Some(std::sync::Arc::new(crate::ProviderWirePolicy::default())));
        let bytes = response.bytes_stream().map_err(ProviderError::from)?;
        Ok(crate::wire::shared_stream(
            bytes,
            adapter,
            Some(CHUNK_TIMEOUT),
            policy,
        ))
    }

    /// Discover models, then keep only wire-resolved ones selectable.
    ///
    /// The upstream list remains the availability source, but a model with no
    /// reviewed wire hint is omitted rather than advertised and later defaulted
    /// to Chat. A later shared-profile update makes it selectable without any
    /// CodeGG source change.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let discovery = self.discovery.as_ref().ok_or_else(|| {
            ProviderError::api(
                "provider_profile_contract",
                "shared provider profile has no entry for this provider",
            )
        })?;
        let discovered = discovery.models().await?;
        Ok(qualify_models(&discovered, &self.id))
    }
}

/// Filter discovered models down to those the shared profile can execute.
pub fn qualify_models(discovered: &[ModelInfo], provider_id: &str) -> Vec<ModelInfo> {
    discovered
        .iter()
        .filter(|model| provider_profile::is_wire_resolved(provider_id, &model.id))
        .cloned()
        .collect()
}

/// Model ids that were discovered but are not wire-resolved.
///
/// Retained for diagnostics so an unresolved model is explainable rather than
/// silently absent. Never used to build a request.
pub fn unresolved_models(discovered: &[ModelInfo], provider_id: &str) -> Vec<String> {
    discovered
        .iter()
        .filter(|model| !provider_profile::is_wire_resolved(provider_id, &model.id))
        .map(|model| model.id.clone())
        .collect()
}

/// Base URL the shared profile owns for this provider.
pub fn shared_base_url() -> Option<String> {
    provider_profile::shared_base_url("opencode_go")
}
/// End-to-end capture tests over a real socket.
///
/// These assert the observable contract the plan requires: the exact path per
/// surface, the exact credential header per surface, the stable session header
/// on all three, the request grammar per surface, and that an unresolved model
/// produces zero network I/O.
#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::{ContentPart, Message, ProviderRequestContext, ToolDefinition};
    use futures_util::StreamExt;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    struct Capture {
        request_line: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Capture {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }

        fn has(&self, name: &str) -> bool {
            self.header(name).is_some()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Capture {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            match stream.read(&mut chunk) {
                Ok(0) => panic!("client closed before sending headers"),
                Ok(read) => {
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(position) = find_subslice(&buffer, b"\r\n\r\n") {
                        break position;
                    }
                }
                Err(error) => panic!("read request: {error}"),
            }
        };
        let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default().to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| {
                line.split_once(':').map(|(name, value)| {
                    (name.trim().to_ascii_lowercase(), value.trim().to_string())
                })
            })
            .collect();
        let content_length = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .and_then(|(_, value)| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = buffer[header_end + 4..].to_vec();
        while body.len() < content_length {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => body.extend_from_slice(&chunk[..read]),
                Err(error) => panic!("read body: {error}"),
            }
        }
        Capture {
            request_line,
            headers,
            body: String::from_utf8_lossy(&body).to_string(),
        }
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    /// Single-shot capture server returning one canned SSE response.
    fn spawn_capture(response_body: &'static str) -> (String, std::thread::JoinHandle<Capture>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let addr = listener.local_addr().expect("capture address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let capture = read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            )
            .expect("write headers");
            stream
                .write_all(response_body.as_bytes())
                .expect("write body");
            stream.flush().ok();
            capture
        });
        (format!("http://{addr}"), handle)
    }

    /// Capture server returning a bare HTTP status, for auth/transport cases.
    fn spawn_status(
        status: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<Capture>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind status server");
        let addr = listener.local_addr().expect("capture address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let capture = read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write response");
            stream.flush().ok();
            capture
        });
        (format!("http://{addr}"), handle)
    }

    /// `EventStream` is not `Debug`, so failures are unwrapped explicitly.
    fn expect_stream_error(
        result: Result<EventStream, ProviderError>,
        context: &str,
    ) -> ProviderError {
        match result {
            Ok(_) => panic!("expected a failure: {context}"),
            Err(error) => error,
        }
    }

    fn provider_at(base: &str) -> OpenCodeGoProvider {
        OpenCodeGoProvider::new(Credential::api_key("sk-test-secret-value")).with_capture_base(base)
    }

    fn request(model: &str, session: Option<&str>) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            }],
            model: model.to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: ProviderRequestContext {
                session_id: session.map(Arc::from),
                ..Default::default()
            },
        }
    }

    const CHAT_SSE: &str =
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    const RESPONSES_SSE: &str = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"usage\":",
        "{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );
    const MESSAGES_SSE: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"minimax-m3\",",
        "\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    #[tokio::test]
    async fn responses_model_uses_responses_path_bearer_and_stable_session() {
        let (base, handle) = spawn_capture(RESPONSES_SSE);
        let mut events = provider_at(&base)
            .stream(&request("gpt-6-luna", Some("SESSION-A")))
            .await
            .expect("responses request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        assert!(
            capture.request_line.contains("POST /responses "),
            "unexpected path: {}",
            capture.request_line
        );
        assert_eq!(
            capture.header("authorization"),
            Some("Bearer sk-test-secret-value")
        );
        assert!(
            !capture.has("x-api-key"),
            "Responses must not send x-api-key"
        );
        assert_eq!(capture.header("x-opencode-session"), Some("SESSION-A"));
        let body: serde_json::Value = serde_json::from_str(&capture.body).expect("json body");
        assert!(body.get("input").is_some(), "Responses grammar uses input");
        assert!(body.get("messages").is_none());
        assert_eq!(body["model"], "gpt-6-luna");
    }

    #[tokio::test]
    async fn muse_responses_model_uses_the_same_responses_surface() {
        let (base, handle) = spawn_capture(RESPONSES_SSE);
        let mut events = provider_at(&base)
            .stream(&request("muse-spark-1.3-contributor", Some("S")))
            .await
            .expect("responses request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        assert!(capture.request_line.contains("POST /responses "));
        assert_eq!(capture.header("x-opencode-session"), Some("S"));
    }

    #[tokio::test]
    async fn chat_model_uses_chat_completions_path_bearer_and_stable_session() {
        let (base, handle) = spawn_capture(CHAT_SSE);
        let mut events = provider_at(&base)
            .stream(&request("glm-5.3-flash", Some("SESSION-B")))
            .await
            .expect("chat request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        assert!(
            capture.request_line.contains("POST /chat/completions "),
            "unexpected path: {}",
            capture.request_line
        );
        assert_eq!(
            capture.header("authorization"),
            Some("Bearer sk-test-secret-value")
        );
        assert!(!capture.has("x-api-key"), "Chat must not send x-api-key");
        assert_eq!(capture.header("x-opencode-session"), Some("SESSION-B"));
        let body: serde_json::Value = serde_json::from_str(&capture.body).expect("json body");
        assert!(body.get("messages").is_some(), "Chat grammar uses messages");
        assert!(body.get("input").is_none());
    }

    #[tokio::test]
    async fn messages_model_uses_messages_path_and_api_key_auth() {
        let (base, handle) = spawn_capture(MESSAGES_SSE);
        let mut events = provider_at(&base)
            .stream(&request("minimax-m3", Some("SESSION-C")))
            .await
            .expect("messages request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        assert!(
            capture.request_line.contains("POST /messages "),
            "unexpected path: {}",
            capture.request_line
        );
        // Messages carries the credential as x-api-key, never as Bearer.
        assert_eq!(capture.header("x-api-key"), Some("sk-test-secret-value"));
        assert!(
            !capture.has("authorization"),
            "Messages must not send a Bearer credential"
        );
        assert_eq!(capture.header("x-opencode-session"), Some("SESSION-C"));
        let body: serde_json::Value = serde_json::from_str(&capture.body).expect("json body");
        assert!(
            body.get("messages").is_some(),
            "Messages grammar uses messages"
        );
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["model"], "minimax-m3");
    }

    #[tokio::test]
    async fn qwen_messages_model_uses_the_messages_surface() {
        let (base, handle) = spawn_capture(MESSAGES_SSE);
        let mut events = provider_at(&base)
            .stream(&request("qwen3.8-max", Some("S")))
            .await
            .expect("messages request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        assert!(capture.request_line.contains("POST /messages "));
        assert_eq!(capture.header("x-api-key"), Some("sk-test-secret-value"));
    }

    #[tokio::test]
    async fn session_identity_is_stable_and_isolated_per_request() {
        for session in ["S1", "S2"] {
            let (base, handle) = spawn_capture(CHAT_SSE);
            let mut events = provider_at(&base)
                .stream(&request("glm-5.3-flash", Some(session)))
                .await
                .expect("chat request");
            while let Some(event) = events.next().await {
                event.expect("stream event");
            }
            let capture = handle.join().expect("capture joins");
            assert_eq!(capture.header("x-opencode-session"), Some(session));
            assert!(
                !capture.body.contains("x-opencode-session"),
                "the session header must never be duplicated into the body"
            );
        }
    }

    #[tokio::test]
    async fn missing_session_context_fails_locally_before_any_network_io() {
        // No capture server is bound: a request that reached the network would
        // fail to connect rather than return this typed error.
        let provider = OpenCodeGoProvider::new(Credential::api_key("sk-test-secret-value"))
            .with_capture_base("http://127.0.0.1:1");
        let error = expect_stream_error(
            provider.stream(&request("glm-5.3-flash", None)).await,
            "missing session must fail",
        );
        // Same typed code the compatible provider emits for this condition.
        assert!(
            matches!(
                &error,
                ProviderError::Api { code, .. } if code == "missing_session_context"
            ),
            "unexpected error: {error}"
        );
        assert_ne!(error.error_class(), "auth");
        assert!(error.to_string().contains("session"));
    }

    #[tokio::test]
    async fn unresolved_model_sends_no_request_and_never_defaults_to_chat() {
        // Port 1 is not listening, so any outbound attempt would surface as a
        // transport error rather than the metadata error asserted here.
        let provider = OpenCodeGoProvider::new(Credential::api_key("sk-test-secret-value"))
            .with_capture_base("http://127.0.0.1:1");
        let error = expect_stream_error(
            provider
                .stream(&request("some-unreviewed-model", Some("S")))
                .await,
            "unresolved model must fail",
        );
        // A metadata failure is never an authentication failure: M010's
        // credential axis is owned by real inference feedback.
        assert_eq!(error.error_class(), "model_not_found");
        assert_ne!(error.error_class(), "auth");
        assert!(error.to_string().contains("some-unreviewed-model"));
    }

    #[tokio::test]
    async fn unauthorized_response_reaches_the_typed_auth_classification() {
        let (base, handle) = spawn_status("401 Unauthorized", r#"{"error":"bad key"}"#);
        let error = expect_stream_error(
            provider_at(&base)
                .stream(&request("glm-5.3-flash", Some("S")))
                .await,
            "401 must fail",
        );
        let _ = handle.join();
        assert_eq!(error.error_class(), "auth");
    }

    #[tokio::test]
    async fn forbidden_response_also_reaches_auth_classification() {
        let (base, handle) = spawn_status("403 Forbidden", r#"{"error":"forbidden"}"#);
        let error = expect_stream_error(
            provider_at(&base)
                .stream(&request("minimax-m3", Some("S")))
                .await,
            "403 must fail",
        );
        let _ = handle.join();
        assert_eq!(error.error_class(), "auth");
    }

    #[tokio::test]
    async fn server_error_is_not_misreported_as_an_auth_failure() {
        let (base, handle) = spawn_status("500 Internal Server Error", r#"{"error":"boom"}"#);
        let error = expect_stream_error(
            provider_at(&base)
                .stream(&request("gpt-6-luna", Some("S")))
                .await,
            "500 must fail",
        );
        let _ = handle.join();
        assert_ne!(error.error_class(), "auth");
    }

    #[tokio::test]
    async fn secrets_are_absent_from_error_renderings() {
        let (base, handle) = spawn_status("401 Unauthorized", r#"{"error":"bad key"}"#);
        let error = expect_stream_error(
            provider_at(&base)
                .stream(&request("glm-5.3-flash", Some("S")))
                .await,
            "401 must fail",
        );
        let _ = handle.join();
        let rendered = format!("{error}");
        assert!(
            !rendered.contains("sk-test-secret-value"),
            "credential leaked into error text: {rendered}"
        );
    }

    #[tokio::test]
    async fn tools_survive_the_responses_surface_request_grammar() {
        let (base, handle) = spawn_capture(RESPONSES_SSE);
        let mut req = request("gpt-6-luna", Some("S"));
        req.tools = Some(vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "read".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            defer_loading: None,
        }]);
        let mut events = provider_at(&base)
            .stream(&req)
            .await
            .expect("responses request");
        while let Some(event) = events.next().await {
            event.expect("stream event");
        }
        let capture = handle.join().expect("capture joins");
        let body: serde_json::Value = serde_json::from_str(&capture.body).expect("json body");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "read_file");
    }

    #[tokio::test]
    async fn one_logical_request_uses_exactly_one_surface() {
        // Across all three surfaces, each request produced exactly one request
        // line on the capture server; there is no cross-surface retry.
        for (model, expected_path, sse) in [
            ("gpt-6-luna", "/responses", RESPONSES_SSE),
            ("glm-5.3-flash", "/chat/completions", CHAT_SSE),
            ("minimax-m3", "/messages", MESSAGES_SSE),
        ] {
            let (base, handle) = spawn_capture(sse);
            let mut events = provider_at(&base)
                .stream(&request(model, Some("S")))
                .await
                .expect("request");
            while let Some(event) = events.next().await {
                event.expect("stream event");
            }
            let capture = handle.join().expect("capture joins");
            assert!(
                capture
                    .request_line
                    .contains(&format!("POST {expected_path} ")),
                "{model} used {} instead of {expected_path}",
                capture.request_line
            );
        }
    }
}

/// Catalog qualification tests (M011 WP-D).
#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::ModelInfo;

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            provider: "opencode_go".to_string(),
            context_window: 0,
            max_output_tokens: None,
            supports_tools: false,
            supports_vision: false,
            variants: Vec::new(),
        }
    }

    #[test]
    fn wire_resolved_models_survive_and_unresolved_ones_are_dropped() {
        let discovered = vec![
            model("gpt-6-luna"),
            model("glm-5.3-flash"),
            model("minimax-m3"),
            model("brand-new-unreviewed-model"),
        ];
        let selectable = qualify_models(&discovered, "opencode_go");
        let ids: Vec<&str> = selectable.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["gpt-6-luna", "glm-5.3-flash", "minimax-m3"]);
    }

    #[test]
    fn unresolved_discovered_models_are_retained_for_diagnostics_only() {
        let discovered = vec![model("brand-new-unreviewed-model"), model("gpt-6-luna")];
        let unresolved = unresolved_models(&discovered, "opencode_go");
        assert_eq!(unresolved, vec!["brand-new-unreviewed-model"]);
    }

    #[test]
    fn qualification_is_deterministic_across_repeated_calls() {
        let discovered = vec![model("gpt-6-luna"), model("unknown-x")];
        let first = qualify_models(&discovered, "opencode_go");
        let second = qualify_models(&discovered, "opencode_go");
        assert_eq!(first.len(), second.len());
        assert_eq!(first[0].id, second[0].id);
    }
}
