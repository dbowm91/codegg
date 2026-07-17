//! A small, bounded probe for Eggpool's OpenAI-compatible `/models` endpoint.
//!
//! This module is intentionally independent of connection storage and protocol
//! types.  It is the providers-side seam used by those layers to validate an
//! endpoint and obtain a small, deterministic model catalog.

use futures::StreamExt;
use reqwest::redirect::Policy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use url::Url;

pub const EGGPOOL_DEFAULT_PORT: u16 = 11_300;
const DEFAULT_RESPONSE_BYTE_LIMIT: usize = 1024 * 1024;
const DEFAULT_MODEL_COUNT_LIMIT: usize = 256;
const DEFAULT_MODEL_STRING_LIMIT: usize = 256;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Stable, redacted categories for probe failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EggpoolProbeReasonCode {
    Auth,
    Unreachable,
    Timeout,
    Tls,
    InvalidJson,
    Unsupported,
    Redirect,
    Empty,
    Oversized,
    Cancelled,
    InvalidInput,
}

impl EggpoolProbeReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Tls => "tls",
            Self::InvalidJson => "invalid_json",
            Self::Unsupported => "unsupported",
            Self::Redirect => "redirect_disallowed",
            Self::Empty => "empty",
            Self::Oversized => "oversized",
            Self::Cancelled => "cancelled",
            Self::InvalidInput => "invalid_input",
        }
    }
}

impl fmt::Display for EggpoolProbeReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A redacted probe error.  It deliberately carries no reqwest or serde
/// source error because those can contain a URL, response text, or headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EggpoolProbeError {
    reason: EggpoolProbeReasonCode,
}

impl EggpoolProbeError {
    const fn new(reason: EggpoolProbeReasonCode) -> Self {
        Self { reason }
    }

    pub const fn reason_code(self) -> EggpoolProbeReasonCode {
        self.reason
    }
}

impl fmt::Display for EggpoolProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "eggpool probe failed: {}", self.reason)
    }
}

impl std::error::Error for EggpoolProbeError {}

/// API-key input kept behind a type whose debug and display forms are safe.
pub struct EggpoolApiKey(String);

impl EggpoolApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<String> for EggpoolApiKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for EggpoolApiKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for EggpoolApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EggpoolApiKey(<redacted>)")
    }
}

impl fmt::Display for EggpoolApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Cancellation handle for an in-flight probe.
#[derive(Clone, Debug, Default)]
pub struct EggpoolCancellationToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl EggpoolCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Resource bounds and timeout controls for one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolProbeOptions {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub overall_timeout: Duration,
    pub response_byte_limit: usize,
    pub model_count_limit: usize,
    pub model_string_limit: usize,
}

impl Default for EggpoolProbeOptions {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            overall_timeout: DEFAULT_OVERALL_TIMEOUT,
            response_byte_limit: DEFAULT_RESPONSE_BYTE_LIMIT,
            model_count_limit: DEFAULT_MODEL_COUNT_LIMIT,
            model_string_limit: DEFAULT_MODEL_STRING_LIMIT,
        }
    }
}

/// A bounded, stable projection of one discovered model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolModelSummary {
    pub id: String,
    pub name: String,
}

/// Successful probe output.  The digest is computed from the sorted bounded
/// summaries, so response order and irrelevant provider fields do not matter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EggpoolProbeSummary {
    pub models: Vec<EggpoolModelSummary>,
    pub digest: String,
}

/// Normalize an Eggpool/OpenAI-compatible base URL.
///
/// Host-only input is accepted as HTTP and uses Eggpool's default port.  A
/// supplied scheme, port, and path are preserved after removing trailing
/// slashes.  Userinfo, query strings, fragments, control characters, and path
/// traversal are rejected before any network request is possible.
pub fn normalize_eggpool_base_url(input: &str) -> Result<String, EggpoolProbeError> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    if has_path_traversal(&candidate) {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    let mut url = Url::parse(&candidate)
        .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;

    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    if url
        .path_segments()
        .map(|mut segments| segments.any(|segment| segment == ".."))
        .unwrap_or(false)
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }

    if url.port().is_none() {
        url.set_port(Some(EGGPOOL_DEFAULT_PORT))
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;
    }

    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// A reqwest-backed Eggpool probe.  The API key is private and has no public
/// accessor, preventing accidental inclusion in redacted result objects.
pub struct EggpoolProbe {
    base_url: String,
    api_key: EggpoolApiKey,
    options: EggpoolProbeOptions,
    client: reqwest::Client,
}

impl fmt::Debug for EggpoolProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EggpoolProbe")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl EggpoolProbe {
    pub fn new(
        base_url: impl AsRef<str>,
        api_key: impl Into<EggpoolApiKey>,
        options: EggpoolProbeOptions,
    ) -> Result<Self, EggpoolProbeError> {
        validate_options(&options)?;
        let base_url = normalize_eggpool_base_url(base_url.as_ref())?;
        let api_key = api_key.into();
        if api_key.0.chars().any(char::is_control) {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(options.connect_timeout)
            .timeout(options.request_timeout)
            .redirect(Policy::none())
            .build()
            .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput))?;

        Ok(Self {
            base_url,
            api_key,
            options,
            client,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn probe(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        match tokio::time::timeout(self.options.overall_timeout, self.probe_inner(cancellation))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Timeout)),
        }
    }

    async fn probe_inner(
        &self,
        cancellation: &EggpoolCancellationToken,
    ) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let url = format!("{}/models", self.base_url);
        let mut request = self.client.get(url);
        if !self.api_key.0.is_empty() {
            let value = format!("Bearer {}", self.api_key.0);
            request = request
                .header(reqwest::header::AUTHORIZATION, value)
                .header(reqwest::header::ACCEPT, "application/json");
        }

        let response = select_cancel(cancellation, request.send()).await?;

        if cancellation.is_cancelled() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled));
        }

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Auth));
        }
        if status.is_redirection() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Redirect));
        }
        if !status.is_success() {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported));
        }

        if response
            .content_length()
            .is_some_and(|length| length > self.options.response_byte_limit as u64)
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }

        let body =
            read_body_bounded(response, self.options.response_byte_limit, cancellation).await?;
        parse_summary(&body, &self.options)
    }
}

fn validate_options(options: &EggpoolProbeOptions) -> Result<(), EggpoolProbeError> {
    if options.connect_timeout.is_zero()
        || options.request_timeout.is_zero()
        || options.overall_timeout.is_zero()
        || options.response_byte_limit == 0
        || options.model_count_limit == 0
        || options.model_string_limit == 0
    {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidInput));
    }
    Ok(())
}

fn has_path_traversal(candidate: &str) -> bool {
    let Some((_, authority_and_path)) = candidate.split_once("://") else {
        return false;
    };
    let Some(path_start) = authority_and_path.find('/') else {
        return false;
    };
    let path = authority_and_path[path_start..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    path.split('/').any(|segment| {
        segment == ".."
            || segment.eq_ignore_ascii_case("%2e%2e")
            || segment.eq_ignore_ascii_case("%2e.%2e")
            || segment.eq_ignore_ascii_case(".%2e")
    })
}

async fn select_cancel<F, T>(
    cancellation: &EggpoolCancellationToken,
    operation: F,
) -> Result<T, EggpoolProbeError>
where
    F: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    tokio::select! {
        _ = cancellation.cancelled() => Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled)),
        result = operation => result.map_err(classify_request_error),
    }
}

async fn read_body_bounded(
    response: reqwest::Response,
    limit: usize,
    cancellation: &EggpoolCancellationToken,
) -> Result<Vec<u8>, EggpoolProbeError> {
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::select! {
        _ = cancellation.cancelled() => return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Cancelled)),
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk.map_err(classify_body_error)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_request_error(error: reqwest::Error) -> EggpoolProbeError {
    if error.is_timeout() {
        return EggpoolProbeError::new(EggpoolProbeReasonCode::Timeout);
    }
    if looks_like_tls_error(&error) {
        return EggpoolProbeError::new(EggpoolProbeReasonCode::Tls);
    }
    EggpoolProbeError::new(EggpoolProbeReasonCode::Unreachable)
}

fn classify_body_error(error: reqwest::Error) -> EggpoolProbeError {
    classify_request_error(error)
}

fn looks_like_tls_error(error: &reqwest::Error) -> bool {
    // Classification only; this string is never returned or logged.
    let text = error.to_string().to_ascii_lowercase();
    [
        "tls",
        "ssl",
        "certificate",
        "handshake",
        "rustls",
        "invalid peer",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn parse_summary(
    body: &[u8],
    options: &EggpoolProbeOptions,
) -> Result<EggpoolProbeSummary, EggpoolProbeError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| EggpoolProbeError::new(EggpoolProbeReasonCode::InvalidJson))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported))?;

    if data.is_empty() {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Empty));
    }
    if data.len() > options.model_count_limit {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
    }

    let mut models = BTreeMap::new();
    for item in data {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| EggpoolProbeError::new(EggpoolProbeReasonCode::Unsupported))?;
        let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
        if id.chars().count() > options.model_string_limit
            || name.chars().count() > options.model_string_limit
        {
            return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Oversized));
        }

        // Duplicate IDs are one catalog entry.  BTreeMap ordering gives a
        // stable winner when a server returns duplicate IDs with names.
        models
            .entry(id.to_owned())
            .and_modify(|existing: &mut String| {
                if name < existing.as_str() {
                    *existing = name.to_owned();
                }
            })
            .or_insert_with(|| name.to_owned());
    }

    if models.is_empty() {
        return Err(EggpoolProbeError::new(EggpoolProbeReasonCode::Empty));
    }

    let models = models
        .into_iter()
        .map(|(id, name)| EggpoolModelSummary { id, name })
        .collect::<Vec<_>>();
    let digest = digest_models(&models);
    Ok(EggpoolProbeSummary { models, digest })
}

fn digest_models(models: &[EggpoolModelSummary]) -> String {
    let mut hasher = Sha256::new();
    for model in models {
        hash_string(&mut hasher, &model.id);
        hash_string(&mut hasher, &model.name);
    }
    hex::encode(hasher.finalize())
}

fn hash_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::OnceLock;
    use std::thread;

    struct FakeServer {
        url: String,
        join: thread::JoinHandle<(String, String)>,
    }

    static FAKE_SERVER_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    async fn fake_server_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        FAKE_SERVER_TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    fn fake_server(status: u16, body: &'static str, delay: Duration) -> Option<FakeServer> {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            // Some restricted test sandboxes disallow local listeners.  The
            // deterministic integration coverage remains active where local
            // sockets are available.
            Err(_) => return None,
        };
        let address = listener.local_addr().expect("fake server address");
        let join = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fake request");
            let request = read_request(&mut stream);
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let reason = if status == 200 { "OK" } else { "ERROR" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let auth = request
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("authorization").then_some(value)
                })
                .unwrap_or_default()
                .trim()
                .to_owned();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_owned();
            (auth, path)
        });
        Some(FakeServer {
            url: format!("http://{address}"),
            join,
        })
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let count = stream.read(&mut chunk).expect("read fake request");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(bytes).expect("fake request is HTTP text")
    }

    fn probe(server: &FakeServer, key: &str, options: EggpoolProbeOptions) -> EggpoolProbe {
        EggpoolProbe::new(&server.url, key, options).expect("valid probe")
    }

    #[test]
    fn normalizes_host_port_and_rejects_secret_bearing_urls() {
        assert_eq!(
            normalize_eggpool_base_url("127.0.0.1"),
            Ok("http://127.0.0.1:11300".to_string())
        );
        assert_eq!(
            normalize_eggpool_base_url("https://[::1]:1234/v1/"),
            Ok("https://[::1]:1234/v1".to_string())
        );
        for value in [
            "https://user:pass@example.test",
            "https://example.test?key=secret",
            "https://example.test/../private",
            "http://example.test:0",
            "ftp://example.test",
        ] {
            assert_eq!(
                normalize_eggpool_base_url(value)
                    .expect_err("input should be rejected")
                    .reason_code(),
                EggpoolProbeReasonCode::InvalidInput
            );
        }
    }

    #[tokio::test]
    async fn probes_models_with_auth_and_stable_summary() {
        let _lock = fake_server_test_lock().await;
        let Some(server) = fake_server(
            200,
            r#"{"data":[{"id":"zeta"},{"id":"alpha","name":"Alpha"},{"id":"zeta","name":"Zeta"}]}"#,
            Duration::ZERO,
        ) else {
            return;
        };
        let probe = probe(&server, "test-api-key", EggpoolProbeOptions::default());
        let cancellation = EggpoolCancellationToken::new();
        let summary = probe.probe(&cancellation).await.expect("probe succeeds");

        assert_eq!(probe.base_url(), server.url);
        assert_eq!(
            summary.models,
            vec![
                EggpoolModelSummary {
                    id: "alpha".to_string(),
                    name: "Alpha".to_string(),
                },
                EggpoolModelSummary {
                    id: "zeta".to_string(),
                    name: "Zeta".to_string(),
                },
            ]
        );
        assert_eq!(summary.digest, digest_models(&summary.models));
        let (auth, path) = server.join.join().expect("fake server joins");
        assert_eq!(auth, "Bearer test-api-key");
        assert_eq!(path, "/models");
    }

    #[tokio::test]
    async fn returns_redacted_reason_codes_for_response_shapes_and_limits() {
        let _lock = fake_server_test_lock().await;
        let cases = [
            (
                401,
                r#"{"error":"the-key-must-not-leak"}"#,
                EggpoolProbeReasonCode::Auth,
            ),
            (
                404,
                r#"{"error":"not-openai-compatible"}"#,
                EggpoolProbeReasonCode::Unsupported,
            ),
            (
                302,
                r#"{"location":"https://elsewhere.invalid"}"#,
                EggpoolProbeReasonCode::Redirect,
            ),
            (200, "not-json", EggpoolProbeReasonCode::InvalidJson),
            (200, r#"{"data":[]}"#, EggpoolProbeReasonCode::Empty),
        ];
        for (status, body, expected) in cases {
            let Some(server) = fake_server(status, body, Duration::ZERO) else {
                return;
            };
            let probe = probe(
                &server,
                "the-key-must-not-leak",
                EggpoolProbeOptions::default(),
            );
            let error = probe
                .probe(&EggpoolCancellationToken::new())
                .await
                .expect_err("probe should fail");
            assert_eq!(error.reason_code(), expected);
            assert!(!error.to_string().contains("the-key-must-not-leak"));
            server.join.join().expect("fake server joins");
        }

        let Some(server) = fake_server(200, r#"{"data":[{"id":"long"}]}"#, Duration::ZERO) else {
            return;
        };
        let options = EggpoolProbeOptions {
            response_byte_limit: 8,
            ..EggpoolProbeOptions::default()
        };
        let error = probe(&server, "key", options)
            .probe(&EggpoolCancellationToken::new())
            .await
            .expect_err("response should be bounded");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Oversized);
        server.join.join().expect("fake server joins");
    }

    #[tokio::test]
    async fn cancellation_and_overall_timeout_are_bounded() {
        let _lock = fake_server_test_lock().await;
        let Some(server) = fake_server(
            200,
            r#"{"data":[{"id":"slow"}]}"#,
            Duration::from_millis(250),
        ) else {
            return;
        };
        let options = EggpoolProbeOptions {
            overall_timeout: Duration::from_millis(25),
            request_timeout: Duration::from_secs(1),
            ..EggpoolProbeOptions::default()
        };
        let error = probe(&server, "key", options)
            .probe(&EggpoolCancellationToken::new())
            .await
            .expect_err("overall timeout should win");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Timeout);
        server.join.join().expect("fake server joins");

        let token = EggpoolCancellationToken::new();
        token.cancel();
        let error = EggpoolProbe::new("127.0.0.1", "key", EggpoolProbeOptions::default())
            .expect("valid probe")
            .probe(&token)
            .await
            .expect_err("cancelled probe should not request");
        assert_eq!(error.reason_code(), EggpoolProbeReasonCode::Cancelled);
    }

    #[test]
    fn secret_is_redacted_from_debug_and_display() {
        let key = EggpoolApiKey::new("super-secret-api-key");
        assert!(!format!("{key:?}").contains("super-secret-api-key"));
        assert!(!key.to_string().contains("super-secret-api-key"));
    }

    #[test]
    fn redirect_reason_is_stable_and_redacted() {
        assert_eq!(
            EggpoolProbeReasonCode::Redirect.as_str(),
            "redirect_disallowed"
        );
        let error = EggpoolProbeError::new(EggpoolProbeReasonCode::Redirect);
        assert!(!error.to_string().contains("Location"));
    }
}
