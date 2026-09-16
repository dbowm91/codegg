use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider not found: {0}")]
    NotFound(String),

    #[error("api error: {code}: {message}")]
    Api {
        code: String,
        message: String,
        url: String,
    },

    #[error("stream error: {0}")]
    Stream(String),

    #[error("rate limit exceeded")]
    RateLimit,

    #[error("rate limit exceeded")]
    RateLimited { retry_after: Option<Duration> },

    #[error("transport error ({kind})")]
    Transport { kind: String },

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("circuit breaker open: {0}")]
    CircuitOpen(String),
}

/// Retry disposition for a provider failure.
///
/// - `Permanent` failures must not be retried for the unchanged request
///   (auth, invalid request, missing model, policy).
/// - `Transient` failures may be retried within bounded policy
///   (rate limit, timeout, connect/DNS/TLS/IO, 5xx, stream interruption).
/// - `Conditional` failures retry only when an explicit recovery operation
///   changed the underlying condition (circuit half-open probe, credential
///   refresh). The provider-turn retry loop treats them as non-retryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDisposition {
    Permanent,
    Transient,
    Conditional,
}

/// Upper bound applied to server `Retry-After` hints before they influence
/// backoff. Hints above the cap are clamped rather than honored verbatim.
pub const MAX_RETRY_AFTER_HINT: Duration = Duration::from_secs(30);

impl ProviderError {
    pub fn api(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
            url: String::new(),
        }
    }

    pub fn api_with_url(
        code: impl Into<String>,
        message: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
            url: url.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self.retry_disposition(), RetryDisposition::Transient)
    }

    /// Construct a rate-limit error preserving a bounded `Retry-After` hint.
    pub fn rate_limited(retry_after: Option<Duration>) -> Self {
        match retry_after {
            None => Self::RateLimit,
            Some(hint) => Self::RateLimited {
                retry_after: Some(hint.min(MAX_RETRY_AFTER_HINT)),
            },
        }
    }

    /// Build a `Retry-After` hint from a raw header value, clamped to
    /// [`MAX_RETRY_AFTER_HINT`]. Accepts delay-seconds; HTTP-dates are
    /// intentionally unsupported so adapters never block on far-future
    /// server clocks without an explicit deadline policy.
    pub fn parse_retry_after(value: &str) -> Option<Duration> {
        let secs: u64 = value.trim().parse().ok()?;
        Some(Duration::from_secs(secs).min(MAX_RETRY_AFTER_HINT))
    }

    /// Extract a bounded `Retry-After` hint from response headers.
    /// Header lookup is case-insensitive; only the first value is used.
    /// Returns `None` when the header is absent or unparsable.
    pub fn retry_after_from_headers(headers: &http::HeaderMap) -> Option<Duration> {
        headers
            .get(http::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(Self::parse_retry_after)
    }

    /// Secret-safe `Retry-After`-carrying rate-limit error from headers.
    pub fn rate_limit_from_headers(headers: &http::HeaderMap) -> Self {
        Self::rate_limited(Self::retry_after_from_headers(headers))
    }

    /// Bounded server hint carried by this error, if any.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => *retry_after,
            _ => None,
        }
    }

    /// Stable secret-safe class for diagnostics and retry policy.
    /// Never contains credentials, URLs, or response bodies.
    pub fn error_class(&self) -> &'static str {
        match self {
            Self::Auth(_) => "auth",
            Self::ModelNotFound(_) => "model_not_found",
            Self::NotFound(_) => "not_found",
            Self::CircuitOpen(_) => "circuit_open",
            Self::Timeout(_) => "timeout",
            Self::RateLimit | Self::RateLimited { .. } => "rate_limit",
            Self::Stream(_) => "stream_interrupted",
            Self::Transport { kind } => Self::transport_class(kind),
            Self::Api { code, message, .. } => Self::api_class(code, message),
        }
    }

    /// Internal disposition mapping. Public `is_retryable()` is the
    /// compatibility surface; this is the explicit taxonomy.
    pub fn retry_disposition(&self) -> RetryDisposition {
        match self {
            // Permanent for the unchanged request.
            Self::Auth(_) | Self::ModelNotFound(_) | Self::NotFound(_) => {
                RetryDisposition::Permanent
            }
            // Conditional: only an explicit recovery (half-open probe
            // admission, credential refresh) may retry.
            Self::CircuitOpen(_) => RetryDisposition::Conditional,
            Self::Timeout(_) => RetryDisposition::Transient,
            Self::RateLimit | Self::RateLimited { .. } => RetryDisposition::Transient,
            Self::Stream(_) => RetryDisposition::Transient,
            Self::Transport { kind } => {
                if is_transient_transport_kind(kind) {
                    RetryDisposition::Transient
                } else {
                    RetryDisposition::Permanent
                }
            }
            Self::Api { code, message, .. } => {
                if is_transient_api_error(code, message) {
                    RetryDisposition::Transient
                } else {
                    RetryDisposition::Permanent
                }
            }
        }
    }

    /// Map an HTTP failure status to the canonical error shape.
    /// 401/403 become permanent `Auth`; 429 becomes rate limit;
    /// 5xx/408 become transient `Api` with the numeric status preserved;
    /// all other statuses become permanent `Api`. The body is truncated
    /// to a bounded secret-safe prefix by the caller contract; this
    /// helper never attaches URLs.
    pub fn from_http_status(status: u16, body: impl Into<String>) -> Self {
        let body = body.into();
        let preview: String = body.chars().take(500).collect();
        match status {
            401 | 403 => Self::Auth(format!("HTTP {status}: {preview}")),
            429 => Self::RateLimit,
            _ => Self::api(status.to_string(), format!("HTTP {status}: {preview}")),
        }
    }

    fn transport_class(kind: &str) -> &'static str {
        match kind {
            "connect" | "pool" | "proxy_connect" => "connect",
            "tls" => "tls",
            "io" | "hyper" | "hyper_client" => "io",
            "timeout_connect"
            | "timeout_pool"
            | "timeout_proxy_connect"
            | "timeout_proxy_tls"
            | "timeout_write"
            | "timeout_read"
            | "timeout_total" => "timeout",
            "protocol" | "body" | "hyper_client_body" => "stream_interrupted",
            "http2_stream_reset_refused" => "stream_interrupted",
            _ => "transport",
        }
    }

    fn api_class(code: &str, message: &str) -> &'static str {
        if let Ok(status) = code.parse::<u16>() {
            return match status {
                401 | 403 => "auth",
                404 => "model_not_found",
                408 | 425 | 429 => "rate_limit",
                500 | 502 | 503 | 504 => "server",
                400..=499 => "invalid_request",
                _ => "api",
            };
        }
        let lowered = format!("{code} {message}").to_lowercase();
        if lowered.contains("auth")
            || lowered.contains("unauthorized")
            || lowered.contains("forbidden")
            || lowered.contains("invalid_api_key")
            || lowered.contains("incorrect api key")
        {
            "auth"
        } else if lowered.contains("model_not_found")
            || lowered.contains("model not found")
            || (lowered.contains("404") && lowered.contains("model"))
        {
            "model_not_found"
        } else if lowered.contains("invalid_request")
            || lowered.contains("invalid request")
            || lowered.contains("bad request")
        {
            "invalid_request"
        } else if lowered.contains("policy")
            || lowered.contains("content_policy")
            || lowered.contains("moderation")
        {
            "policy"
        } else if lowered.contains("rate") || lowered.contains("429") {
            "rate_limit"
        } else if lowered.contains("timeout") {
            "timeout"
        } else {
            "api"
        }
    }
}

impl From<String> for ProviderError {
    fn from(s: String) -> Self {
        Self::Api {
            code: "unknown".to_string(),
            message: s,
            url: String::new(),
        }
    }
}

impl From<&str> for ProviderError {
    fn from(s: &str) -> Self {
        Self::Api {
            code: "unknown".to_string(),
            message: s.to_string(),
            url: String::new(),
        }
    }
}

impl From<eggfetch_core::Error> for ProviderError {
    fn from(error: eggfetch_core::Error) -> Self {
        if matches!(error, eggfetch_core::Error::Timeout { .. }) {
            return Self::Timeout(error.kind().to_string());
        }
        // Refused-stream resets are safe to replay before the server
        // processed the request; other H2/H3 protocol terminations are not
        // assumed safe.
        if let eggfetch_core::Error::Http2StreamReset { ref reason } = error {
            if reason.starts_with("REFUSED_STREAM") {
                return Self::Transport {
                    kind: "http2_stream_reset_refused".to_string(),
                };
            }
            return Self::api(
                "http2_stream_reset",
                "HTTP transport error (http2_stream_reset)".to_string(),
            );
        }

        let kind = error.kind().to_string();
        // Eggfetch errors intentionally do not cross this boundary verbatim:
        // some request URLs contain credentials (for example Google API keys).
        // The stable category is enough for provider retry/error policy and
        // keeps transport diagnostics free of URL and secret data.
        if is_transient_transport_kind(&kind) {
            return Self::Transport { kind };
        }
        // Permanent local/request-construction failures keep a stable
        // `request_error`-family code so existing classifiers that match on
        // it continue to treat them as non-retryable.
        if kind == "invalid_url" {
            return Self::api("request_error", format!("HTTP transport error ({kind})"));
        }
        Self::api(kind.clone(), format!("HTTP transport error ({kind})"))
    }
}

/// Secret-safe transport kinds eligible for bounded retry.
///
/// DNS resolution surfaces through `connect` in eggfetch; TLS handshake,
/// TCP connect, pool acquisition, hyper IO, body streaming, and refused
/// H2 streams are all transient network conditions. Local configuration
/// failures (bad URL/method/headers, auth header conflicts, TLS identity
/// configuration, certificate verification against the wrong trust roots
/// is treated as permanent here because retrying the unchanged request
/// cannot succeed) stay permanent.
fn is_transient_transport_kind(kind: &str) -> bool {
    matches!(
        kind,
        "connect"
            | "tls"
            | "protocol"
            | "body"
            | "hyper"
            | "hyper_client"
            | "io"
            | "pool"
            | "proxy_connect"
            | "h3_connect"
            | "h3_connection_closed"
            | "h3_stream"
            | "http2_stream_reset_refused"
            | "http2_flow_control"
            | "timeout_connect"
            | "timeout_pool"
            | "timeout_proxy_connect"
            | "timeout_proxy_tls"
            | "timeout_write"
            | "timeout_read"
            | "timeout_total"
    )
}

/// Numeric/status-code API failures eligible for bounded retry.
/// 429 and 408/425/5xx are transient; 401/403 (auth), 400/404/422
/// (invalid request/model), and unknown codes default to permanent so an
/// invalid-request loop cannot retry to exhaustion.
fn is_transient_api_error(code: &str, message: &str) -> bool {
    if let Ok(status) = code.parse::<u16>() {
        return matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504);
    }
    let lowered = format!("{code} {message}").to_lowercase();
    // Word-boundary style checks avoid misclassifying e.g. "moderation"
    // policy text that merely contains "rate" as a substring.
    lowered.contains("rate_limit")
        || lowered.contains("rate limit")
        || lowered.contains("too many requests")
        || lowered.contains("429")
        || lowered.contains("502")
        || lowered.contains("503")
        || lowered.contains("504")
        || lowered.contains("timeout")
        || lowered.contains("temporarily")
        || lowered.contains("try again")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eggfetch_transport_errors_are_secret_safe_and_classified() {
        let error: ProviderError = eggfetch_core::Error::InvalidUrl(
            "https://example.test/models?key=secret-api-key".to_string(),
        )
        .into();
        assert!(matches!(
            error,
            ProviderError::Api { ref code, ref message, ref url }
                if code == "request_error"
                    && message == "HTTP transport error (invalid_url)"
                    && url.is_empty()
        ));
        assert!(!error.to_string().contains("secret-api-key"));
        assert!(!error.is_retryable());
        assert_eq!(error.retry_disposition(), RetryDisposition::Permanent);

        let timeout: ProviderError = eggfetch_core::Error::Timeout {
            phase: eggfetch_core::TimeoutPhase::Connect,
            elapsed: std::time::Duration::from_secs(10),
        }
        .into();
        assert!(matches!(timeout, ProviderError::Timeout(ref phase) if phase == "timeout_connect"));
        assert!(timeout.is_retryable());
    }

    #[test]
    fn retry_taxonomy_matrix() {
        // Permanent: auth, invalid request, model missing, policy-ish, circuit.
        for err in [
            ProviderError::Auth("bad key".to_string()),
            ProviderError::ModelNotFound("nope".to_string()),
            ProviderError::NotFound("nope".to_string()),
            ProviderError::api("400", "HTTP 400: bad request"),
            ProviderError::api("401", "HTTP 401: unauthorized"),
            ProviderError::api("403", "HTTP 403: forbidden"),
            ProviderError::api("404", "HTTP 404: not found"),
            ProviderError::api("422", "HTTP 422: unprocessable"),
            ProviderError::api("invalid_request", "bad request"),
            ProviderError::api("http_error", "API error: policy violation"),
            ProviderError::CircuitOpen("p".to_string()),
        ] {
            assert!(
                !err.is_retryable(),
                "expected permanent/conditional: {} ({})",
                err.error_class(),
                err
            );
            assert!(
                matches!(
                    err.retry_disposition(),
                    RetryDisposition::Permanent | RetryDisposition::Conditional
                ),
                "unexpected disposition for {err}"
            );
        }
        // Transient: rate limit, timeout, 5xx, stream interruption, transport.
        for err in [
            ProviderError::RateLimit,
            ProviderError::rate_limited(Some(Duration::from_secs(2))),
            ProviderError::Timeout("t".to_string()),
            ProviderError::Stream("interrupted".to_string()),
            ProviderError::api("429", "HTTP 429: too many requests"),
            ProviderError::api("500", "HTTP 500: boom"),
            ProviderError::api("502", "HTTP 502: bad gateway"),
            ProviderError::api("503", "HTTP 503: unavailable"),
            ProviderError::api("504", "HTTP 504: gateway timeout"),
            ProviderError::Transport {
                kind: "connect".to_string(),
            },
            ProviderError::Transport {
                kind: "tls".to_string(),
            },
            ProviderError::Transport {
                kind: "io".to_string(),
            },
            ProviderError::Transport {
                kind: "hyper".to_string(),
            },
        ] {
            assert!(
                err.is_retryable(),
                "expected transient: {} ({})",
                err.error_class(),
                err
            );
            assert_eq!(err.retry_disposition(), RetryDisposition::Transient);
        }
        // Auth is permanent even though it used to be retryable.
        let auth = ProviderError::Auth("bad".to_string());
        assert_eq!(auth.error_class(), "auth");
        assert_eq!(auth.retry_disposition(), RetryDisposition::Permanent);
        // Circuit-open is conditional, never blindly retried by the turn loop.
        let open = ProviderError::CircuitOpen("p".to_string());
        assert_eq!(open.retry_disposition(), RetryDisposition::Conditional);
        assert!(!open.is_retryable());
    }

    #[test]
    fn transport_fixture_classifies_transient() {
        for err in [
            eggfetch_core::Error::Connect("refused".to_string()),
            eggfetch_core::Error::Tls("handshake".to_string()),
            eggfetch_core::Error::Io(std::sync::Arc::new(std::io::Error::other("reset"))),
        ] {
            let provider: ProviderError = err.into();
            assert!(
                matches!(provider, ProviderError::Transport { .. }),
                "expected Transport, got {provider}"
            );
            assert!(provider.is_retryable(), "expected retryable: {provider}");
        }
    }

    #[test]
    fn retry_after_parsing_and_cap() {
        assert_eq!(
            ProviderError::parse_retry_after("2"),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            ProviderError::parse_retry_after(" 5 "),
            Some(Duration::from_secs(5))
        );
        assert_eq!(ProviderError::parse_retry_after("nope"), None);
        assert_eq!(
            ProviderError::parse_retry_after("3600"),
            Some(MAX_RETRY_AFTER_HINT)
        );
        let limited = ProviderError::rate_limited(Some(Duration::from_secs(3600)));
        assert_eq!(limited.retry_after(), Some(MAX_RETRY_AFTER_HINT));
        assert!(ProviderError::RateLimit.retry_after().is_none());
    }

    #[test]
    fn http_status_mapping_is_explicit() {
        assert!(matches!(
            ProviderError::from_http_status(401, "unauthorized"),
            ProviderError::Auth(_)
        ));
        assert!(matches!(
            ProviderError::from_http_status(403, "forbidden"),
            ProviderError::Auth(_)
        ));
        assert!(matches!(
            ProviderError::from_http_status(429, "slow down"),
            ProviderError::RateLimit
        ));
        let server = ProviderError::from_http_status(503, "unavailable");
        assert!(server.is_retryable());
        assert_eq!(server.error_class(), "server");
        let bad = ProviderError::from_http_status(400, "bad request");
        assert!(!bad.is_retryable());
        assert!(!ProviderError::from_http_status(400, "x")
            .to_string()
            .contains("secret"));
    }

    #[test]
    fn rate_limited_display_stays_compatible() {
        assert_eq!(ProviderError::RateLimit.to_string(), "rate limit exceeded");
        assert_eq!(
            ProviderError::rate_limited(Some(Duration::from_secs(3))).to_string(),
            "rate limit exceeded"
        );
    }
}

impl From<crate::circuit::CircuitError> for ProviderError {
    fn from(e: crate::circuit::CircuitError) -> Self {
        match e {
            crate::circuit::CircuitError::Open(name) => ProviderError::CircuitOpen(name),
        }
    }
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("llm operation failed: {operation}: {message}")]
    LlmOperation { operation: String, message: String },

    #[error("import error: {0}")]
    Import(String),

    #[error("export error: {0}")]
    Export(String),
}

impl From<sqlx::Error> for StorageError {
    fn from(e: sqlx::Error) -> Self {
        StorageError::Database(e.to_string())
    }
}
