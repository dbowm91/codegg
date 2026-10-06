//! Optional System One-compatible decision backend.
//!
//! This adapter accepts one bounded CodeGG decision per request. It does not
//! expose remote state in diagnostics and never follows redirects.

use std::{
    collections::BTreeMap,
    net::{SocketAddr, ToSocketAddrs},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use codegg_core::decision::{
    BackendCapabilities, BackendState, DecisionAnswer, DecisionEngine, DecisionError,
    DecisionProvenance, DecisionRequest, DecisionResponse, DecisionSpec, DecisionStatus,
    NoopDecisionEngine,
};
use serde::Deserialize;

const MAX_BODY: usize = 64 * 1024;
const MAX_TIMEOUT_MS: u64 = 30_000;
const QUESTION_ID: &str = "codegg_decision_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFieldDiagnostic {
    pub field_name: String,
    pub value_bytes: usize,
}

/// Return payload shape for diagnostics without ever exposing field values.
pub fn privacy_diagnostic(
    request: &DecisionRequest,
) -> Result<Vec<StateFieldDiagnostic>, DecisionError> {
    request.validate()?;
    Ok(request
        .state
        .iter()
        .map(|field| StateFieldDiagnostic {
            field_name: field.key.clone(),
            value_bytes: field.value.len(),
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemOneProfile {
    Reference,
    Ollama,
}

impl SystemOneProfile {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reference" => Ok(Self::Reference),
            "ollama" => Ok(Self::Ollama),
            _ => Err("unknown System One compatibility profile".into()),
        }
    }
}

/// Resolved immutable runtime snapshot. Construct this only after resolving
/// `auth` through CodeGG's `AuthResolver`; no secret is retained in config.
#[derive(Clone)]
pub struct SystemOneConfig {
    pub enabled: bool,
    pub profile: SystemOneProfile,
    pub base_url: String,
    pub model: String,
    pub credential: Option<codegg_providers::Credential>,
    pub timeout: Duration,
    pub discover_models: bool,
}

impl SystemOneConfig {
    pub fn from_schema(
        schema: &codegg_config::schema::DecisionEngineConfig,
        store: Option<std::sync::Arc<codegg_providers::CredentialStore>>,
    ) -> Result<Self, String> {
        if schema.backend != "system_one" {
            return Err("unsupported decision backend kind".into());
        }
        let profile = SystemOneProfile::parse(&schema.profile)?;
        let model = schema.model.clone().unwrap_or_default();
        if schema.enabled
            && (model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control))
        {
            return Err("System One requires a bounded model name".into());
        }
        let base_url = schema.base_url.clone().unwrap_or_else(|| match profile {
            SystemOneProfile::Reference => "https://system-one.dev/v1".into(),
            SystemOneProfile::Ollama => "http://localhost:11434/v1".into(),
        });
        let credential = if schema.enabled {
            if matches!(
                schema.auth,
                Some(
                    codegg_config::schema::AuthConfig::ExternalCommand { .. }
                        | codegg_config::schema::AuthConfig::OAuthDevice { .. }
                )
            ) {
                return Err("System One supports only CodeGG API-key or stored credentials".into());
            }
            let auth = schema.auth.as_ref().map(|auth| match auth {
                codegg_config::schema::AuthConfig::ApiKey {
                    env,
                    value,
                    encrypted_value,
                } => codegg_providers::AuthConfig::ApiKey {
                    env: env.clone(),
                    value: value.clone(),
                    encrypted_value: encrypted_value.clone(),
                },
                codegg_config::schema::AuthConfig::Stored { account_id } => {
                    codegg_providers::AuthConfig::Stored {
                        account_id: account_id.clone(),
                    }
                }
                codegg_config::schema::AuthConfig::ExternalCommand { .. } => {
                    codegg_providers::AuthConfig::ExternalCommand {
                        command: String::new(),
                        args: vec![],
                        timeout_ms: None,
                    }
                }
                codegg_config::schema::AuthConfig::OAuthDevice { .. } => {
                    codegg_providers::AuthConfig::OAuthDevice {
                        client_id: String::new(),
                        scopes: vec![],
                        auth_url: String::new(),
                        token_url: String::new(),
                    }
                }
                codegg_config::schema::AuthConfig::None => codegg_providers::AuthConfig::None,
            });
            let context = codegg_providers::ResolverContext {
                provider_id: "system_one".into(),
                store,
                capability: codegg_providers::CredentialCapability::ApiKeyOrBearer,
                ..Default::default()
            };
            codegg_providers::AuthResolver::new()
                .resolve(auth.as_ref(), &context)
                .map_err(|_| "System One credential unavailable".to_string())?
                .map(|resolved| resolved.credential)
        } else {
            None
        };
        Ok(Self {
            enabled: schema.enabled,
            profile,
            base_url,
            model,
            credential,
            timeout: Duration::from_millis(schema.timeout_ms.clamp(1, MAX_TIMEOUT_MS)),
            discover_models: schema.discover_models,
        })
    }
}

pub struct SystemOneEngine {
    config: SystemOneConfig,
    client: eggfetch_core::Client,
}

impl SystemOneEngine {
    pub fn new(config: SystemOneConfig) -> Self {
        let client = eggfetch_core::Client::builder()
            .timeout(eggfetch_core::Timeout {
                total: Some(config.timeout),
                ..Default::default()
            })
            .follow_redirects(false)
            .build();
        Self { config, client }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    async fn bounded_target(&self, deadline: Instant) -> Result<(String, Vec<SocketAddr>), String> {
        let config = self.config.clone();
        match tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            tokio::task::spawn_blocking(move || validate_target(&config)),
        )
        .await
        {
            Ok(Ok(target)) => target,
            _ => Err("decision endpoint resolution timed out".into()),
        }
    }

    /// Explicit operator-triggered model discovery. No background polling.
    pub async fn discover_models(&self) -> Result<Vec<String>, String> {
        if !self.config.enabled {
            return Err("decision backend is off".into());
        }
        if !self.config.discover_models {
            return Err("model discovery is not explicitly enabled".into());
        }
        let deadline = Instant::now() + self.config.timeout;
        let (base, addresses) = self.bounded_target(deadline).await?;
        let url = format!("{}/models", base.trim_end_matches('/'));
        let mut request = self
            .client
            .get(&url)
            .map_err(|_| "request unavailable".to_string())?
            .timeout(eggfetch_core::Timeout {
                total: Some(self.config.timeout),
                ..Default::default()
            })
            .max_decoded_body_size(MAX_BODY)
            .resolved_addresses(addresses.clone());
        if let Some(credential) = &self.config.credential {
            request = request.header("authorization", &credential.authorization_header_value());
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "model discovery unavailable".to_string())?;
        if !response.is_success() {
            return Err("model discovery unavailable".into());
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| "model discovery unavailable".to_string())?;
        if body.len() > MAX_BODY {
            return Err("model discovery response exceeds limit".into());
        }
        #[derive(Deserialize)]
        struct Models {
            models: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            name: Option<String>,
            id: Option<String>,
        }
        let result: Models = serde_json::from_slice(&body)
            .map_err(|_| "invalid model discovery response".to_string())?;
        Ok(result
            .models
            .into_iter()
            .filter_map(|item| item.name.or(item.id))
            .filter(|name| name.len() <= 128)
            .collect())
    }
}

/// Build the configured engine snapshot. Missing or disabled config resolves
/// to Noop without auth resolution, DNS lookup, or network construction.
pub fn engine_from_config(
    schema: Option<&codegg_config::schema::DecisionEngineConfig>,
    store: Option<std::sync::Arc<codegg_providers::CredentialStore>>,
) -> Result<Box<dyn DecisionEngine>, String> {
    let Some(schema) = schema.filter(|value| value.enabled) else {
        return Ok(Box::new(NoopDecisionEngine));
    };
    Ok(Box::new(SystemOneEngine::new(
        SystemOneConfig::from_schema(schema, store)?,
    )))
}

fn validate_target(config: &SystemOneConfig) -> Result<(String, Vec<SocketAddr>), String> {
    let url = url::Url::parse(&config.base_url)
        .map_err(|_| "invalid decision endpoint URL".to_string())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("decision endpoint URL contains disallowed components".into());
    }
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("unsupported decision endpoint scheme".into());
    }
    if config.profile == SystemOneProfile::Reference && url.scheme() != "https" {
        return Err("reference profile requires HTTPS".into());
    }
    if config.profile == SystemOneProfile::Ollama && url.scheme() != "http" {
        return Err("Ollama profile requires local loopback HTTP".into());
    }
    let host = url
        .host_str()
        .ok_or("decision endpoint URL has no host")?
        .to_ascii_lowercase();
    let port = url
        .port()
        .unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    let addresses: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| "decision endpoint host cannot be resolved".to_string())?
        .collect();
    if addresses.is_empty() {
        return Err("decision endpoint host cannot be resolved".into());
    }
    let loopback_host = host == "localhost" || addresses.iter().all(|a| a.ip().is_loopback());
    match config.profile {
        SystemOneProfile::Ollama if !loopback_host => {
            return Err("Ollama profile requires local loopback HTTP".into())
        }
        _ => {}
    }
    if url.scheme() == "http" && (!loopback_host || config.credential.is_some()) {
        return Err("plain HTTP is limited to unauthenticated loopback endpoints".into());
    }
    if config.profile == SystemOneProfile::Reference
        && addresses
            .iter()
            .any(|a| crate::security::ssrf::is_internal_ip(&a.ip()))
    {
        return Err("reference profile cannot target internal addresses".into());
    }
    Ok((url.as_str().trim_end_matches('/').to_string(), addresses))
}

#[async_trait]
impl DecisionEngine for SystemOneEngine {
    fn state(&self) -> BackendState {
        if !self.config.enabled {
            return BackendState::Off;
        }
        if self.config.model.trim().is_empty() {
            return BackendState::Unavailable("model is not configured".into());
        }
        BackendState::Ready
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            binary: true,
            choice: true,
            score: true,
            rank: false,
            max_options: 128,
            max_candidates: 0,
        }
    }

    async fn decide(
        &self,
        request: DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        request.validate()?;
        if matches!(request.spec, DecisionSpec::Rank { .. }) {
            return Ok(response(
                &request,
                DecisionStatus::Unsupported {
                    reason: "System One does not support CodeGG rank semantics".into(),
                },
                None,
                None,
                0,
            ));
        }
        let unavailable = |reason: &str| {
            response(
                &request,
                DecisionStatus::Unavailable {
                    reason: reason.into(),
                },
                None,
                None,
                0,
            )
        };
        if !self.config.enabled {
            return Ok(unavailable("decision backend is off"));
        }
        if self.config.model.is_empty() {
            return Ok(unavailable("decision model is not configured"));
        }
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(self.config.timeout);
        if remaining.is_zero() {
            return Ok(unavailable("decision deadline expired"));
        }
        let (base, addresses) = match self.bounded_target(deadline).await {
            Ok(v) => v,
            Err(_) => return Ok(unavailable("decision endpoint unavailable")),
        };
        let (question, expected_type, criteria) = match &request.spec {
            DecisionSpec::Binary { question } => (question.clone(), "noul", None),
            DecisionSpec::Choice { question, options } => {
                let mut choices = BTreeMap::new();
                for item in options {
                    choices.insert(item.id.clone(), item.label.clone());
                }
                (
                    question.clone(),
                    "choice",
                    Some(serde_json::Value::Object(
                        choices
                            .into_iter()
                            .map(|(k, v)| (k, serde_json::Value::String(v)))
                            .collect(),
                    )),
                )
            }
            DecisionSpec::Score {
                rubric,
                minimum,
                maximum,
            } => {
                if minimum.fract() != 0.0
                    || maximum.fract() != 0.0
                    || maximum - minimum > 9.0
                    || maximum <= minimum
                {
                    return Ok(response(
                        &request,
                        DecisionStatus::Unsupported {
                            reason: "score range does not fit the supported ordered-level profile"
                                .into(),
                        },
                        None,
                        None,
                        0,
                    ));
                }
                let levels = (*minimum as i64..=*maximum as i64)
                    .map(|v| serde_json::Value::String(format!("{v}: {rubric}")))
                    .collect();
                (
                    rubric.clone(),
                    "score",
                    Some(serde_json::Value::Array(levels)),
                )
            }
            DecisionSpec::Rank { .. } => unreachable!(),
        };
        let mut question_json =
            serde_json::json!({"type": expected_type, "instructions": question});
        if let Some(criteria) = criteria {
            question_json["criteria"] = criteria;
        }
        let state: serde_json::Value = serde_json::Value::Object(
            request
                .state
                .iter()
                .map(|f| (f.key.clone(), serde_json::Value::String(f.value.clone())))
                .collect(),
        );
        let body = serde_json::json!({"model": self.config.model, "state": state, "questions": {QUESTION_ID: question_json}});
        let Ok(body_bytes) = serde_json::to_vec(&body) else {
            return Ok(unavailable("decision request serialization failed"));
        };
        if body_bytes.len() > MAX_BODY {
            return Ok(unavailable("decision request exceeds size limit"));
        }
        let url = format!("{}/systemone", base.trim_end_matches('/'));
        let mut builder = match self.client.post(&url) {
            Ok(b) => b,
            Err(_) => return Ok(unavailable("decision request unavailable")),
        };
        builder = match builder.json(&body) {
            Ok(b) => b,
            Err(_) => return Ok(unavailable("decision request unavailable")),
        }
        .timeout(eggfetch_core::Timeout {
            total: Some(remaining),
            ..Default::default()
        })
        .max_decoded_body_size(MAX_BODY)
        .resolved_addresses(addresses.clone());
        if let Some(credential) = &self.config.credential {
            builder = builder.header("authorization", &credential.authorization_header_value());
        }
        let started = Instant::now();
        let mut wire = match builder.send().await {
            Ok(r) if r.is_success() => r,
            _ => return Ok(unavailable("decision service unavailable")),
        };
        let bytes = match wire.bytes().await {
            Ok(b) if b.len() <= MAX_BODY => b,
            _ => return Ok(unavailable("decision response unavailable")),
        };
        let parsed: WireResponse = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(_) => return Ok(unavailable("invalid decision response")),
        };
        if parsed.model.trim().is_empty()
            || parsed.model.len() > 256
            || parsed.model.chars().any(char::is_control)
            || parsed.answers.len() != 1
        {
            return Ok(unavailable("decision response question set mismatch"));
        }
        let Some(answer) = parsed.answers.get(QUESTION_ID) else {
            return Ok(unavailable("decision response question id mismatch"));
        };
        let mapped = match (&request.spec, answer) {
            (DecisionSpec::Binary { .. }, WireAnswer::Noul { noul })
                if valid_probability(*noul) =>
            {
                DecisionAnswer::Binary {
                    probability_true: *noul,
                    confidence: None,
                }
            }
            (
                DecisionSpec::Choice { options, .. },
                WireAnswer::Choice {
                    choice,
                    probabilities,
                    confidence,
                },
            ) if options.iter().any(|o| o.id == *choice) => {
                let expected: std::collections::BTreeSet<_> =
                    options.iter().map(|o| o.id.as_str()).collect();
                let actual: std::collections::BTreeSet<_> =
                    probabilities.keys().map(String::as_str).collect();
                if actual != expected || probabilities.values().any(|v| !valid_probability(*v)) {
                    return Ok(unavailable("invalid choice probability"));
                }
                let p = probabilities.get(choice).copied();
                if confidence.is_some_and(|v| !valid_probability(v)) {
                    return Ok(unavailable("invalid choice confidence"));
                }
                DecisionAnswer::Choice {
                    option_id: choice.clone(),
                    probability: p,
                }
            }
            (
                DecisionSpec::Score {
                    minimum, maximum, ..
                },
                WireAnswer::Score {
                    score,
                    confidence,
                    probabilities,
                },
            ) if score.is_finite() && *score >= 0.0 && *score <= (*maximum - *minimum) => {
                if confidence.is_some_and(|v| !valid_probability(v)) {
                    return Ok(unavailable("invalid score confidence"));
                }
                let expected_levels = (*maximum - *minimum) as usize + 1;
                if probabilities.len() != expected_levels
                    || probabilities.iter().any(|(key, value)| {
                        key.parse::<usize>()
                            .map_or(true, |index| index >= expected_levels)
                            || !valid_probability(*value)
                    })
                {
                    return Ok(unavailable("invalid score probability distribution"));
                }
                DecisionAnswer::Score {
                    value: minimum + score,
                    confidence: None,
                }
            }
            _ => return Ok(unavailable("decision response type mismatch")),
        };
        let latency = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let result = response(
            &request,
            DecisionStatus::Answered,
            Some(mapped),
            Some(parsed.model),
            latency,
        );
        if result.validate_for(&request).is_err() {
            return Ok(unavailable("decision response failed CodeGG validation"));
        }
        Ok(result)
    }
}

#[derive(Deserialize)]
struct WireResponse {
    model: String,
    answers: BTreeMap<String, WireAnswer>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireAnswer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        confidence: Option<f64>,
    },
    Score {
        score: f64,
        #[serde(default)]
        confidence: Option<f64>,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
    },
}
fn valid_probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn response(
    request: &DecisionRequest,
    status: DecisionStatus,
    answer: Option<DecisionAnswer>,
    model: Option<String>,
    latency_micros: u64,
) -> DecisionResponse {
    DecisionResponse {
        request_id: request.request_id.clone(),
        schema_version: request.schema_version,
        status,
        answer,
        provenance: DecisionProvenance {
            backend: "system_one".into(),
            model,
            runtime_version: Some("system-one-adapter-v1".into()),
            latency_micros,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::decision::{
        DecisionOption, DecisionStatus, StateField, DECISION_SCHEMA_VERSION,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn config(base_url: String, enabled: bool) -> SystemOneConfig {
        SystemOneConfig {
            enabled,
            profile: SystemOneProfile::Ollama,
            base_url,
            model: "nimble".into(),
            credential: None,
            timeout: Duration::from_secs(2),
            discover_models: false,
        }
    }

    fn choice_request() -> DecisionRequest {
        DecisionRequest {
            request_id: "test-request".into(),
            schema_version: DECISION_SCHEMA_VERSION,
            state: vec![StateField {
                key: "kind".into(),
                value: "issue".into(),
            }],
            spec: DecisionSpec::Choice {
                question: "Choose one".into(),
                options: vec![
                    DecisionOption {
                        id: "billing".into(),
                        label: "Billing".into(),
                    },
                    DecisionOption {
                        id: "support".into(),
                        label: "Support".into(),
                    },
                ],
            },
        }
    }

    async fn fixture(
        status: u16,
        response: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        fixture_with_location(status, None, response).await
    }

    async fn fixture_with_location(
        status: u16,
        location: Option<String>,
        response: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake server");
        let address = listener.local_addr().expect("fake address");
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut chunk = [0u8; 2048];
            loop {
                let count = socket.read(&mut chunk).await.expect("read request");
                request.extend_from_slice(&chunk[..count]);
                if let Some(headers_end) = request
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                {
                    let headers = String::from_utf8_lossy(&request[..headers_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let body_end = headers_end + length;
                    if request.len() >= body_end {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let payload = response.as_bytes();
            let location_header = location
                .map(|value| format!("Location: {value}\r\n"))
                .unwrap_or_default();
            let header = format!("HTTP/1.1 {status} Test\r\n{location_header}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
            socket
                .write_all(header.as_bytes())
                .await
                .expect("write headers");
            socket.write_all(payload).await.expect("write response");
            text
        });
        (format!("http://{address}/v1"), task)
    }

    #[test]
    fn profiles_are_explicit_and_endpoint_policy_is_bounded() {
        assert!(SystemOneProfile::parse("reference").is_ok());
        assert!(SystemOneProfile::parse("ollama").is_ok());
        assert!(SystemOneProfile::parse("anything").is_err());
        let local = SystemOneConfig {
            enabled: true,
            profile: SystemOneProfile::Ollama,
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "nimble".into(),
            credential: None,
            timeout: Duration::from_secs(1),
            discover_models: false,
        };
        assert!(validate_target(&local).is_ok());
        let remote_http = SystemOneConfig {
            profile: SystemOneProfile::Reference,
            base_url: "http://example.com/v1".into(),
            ..local
        };
        assert!(validate_target(&remote_http).is_err());
    }

    #[test]
    fn absent_and_default_config_build_noop_engine() {
        let engine = engine_from_config(None, None).expect("absent config");
        assert_eq!(engine.state(), BackendState::Off);
        let engine = engine_from_config(
            Some(&codegg_config::schema::DecisionEngineConfig::default()),
            None,
        )
        .expect("default config");
        assert_eq!(engine.state(), BackendState::Off);
    }

    #[tokio::test]
    async fn choice_round_trip_uses_exact_question_and_option_ids() {
        let (base, server) = fixture(200, r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"choice","choice":"support","probabilities":{"billing":0.1,"support":0.9},"confidence":0.8}}}"#).await;
        let engine = SystemOneEngine::new(config(base, true));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("decision result");
        assert!(matches!(result.status, DecisionStatus::Answered));
        assert!(
            matches!(result.answer, Some(DecisionAnswer::Choice { option_id, probability: Some(p) }) if option_id == "support" && (p - 0.9).abs() < f64::EPSILON)
        );
        let request = server.await.expect("server completion");
        assert!(request.contains("codegg_decision_v1"));
        assert!(request.contains("billing"));
        assert!(request.contains("support"));
    }

    #[tokio::test]
    async fn binary_and_score_round_trip_preserve_supported_values() {
        let (base, server) = fixture(
            200,
            r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"noul","noul":0.7}}}"#,
        )
        .await;
        let engine = SystemOneEngine::new(config(base, true));
        let mut binary = choice_request();
        binary.spec = DecisionSpec::Binary {
            question: "Is this urgent?".into(),
        };
        let result = engine
            .decide(binary, Instant::now() + Duration::from_secs(3))
            .await
            .expect("binary decision");
        assert!(
            matches!(result.answer, Some(DecisionAnswer::Binary { probability_true, confidence: None }) if (probability_true - 0.7).abs() < f64::EPSILON)
        );
        let request = server.await.expect("binary request");
        assert!(request.contains("noul"));

        let (base, server) = fixture(200, r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"score","score":1.5,"probabilities":{"0":0.1,"1":0.3,"2":0.6},"confidence":0.8,"legend":{"0":"low","1":"mid","2":"high"}}}}"#).await;
        let engine = SystemOneEngine::new(config(base, true));
        let mut score = choice_request();
        score.spec = DecisionSpec::Score {
            rubric: "triage urgency".into(),
            minimum: 4.0,
            maximum: 6.0,
        };
        let result = engine
            .decide(score, Instant::now() + Duration::from_secs(3))
            .await
            .expect("score decision");
        assert!(
            matches!(result.answer, Some(DecisionAnswer::Score { value, confidence: None }) if (value - 5.5).abs() < f64::EPSILON)
        );
        let request = server.await.expect("score request");
        assert!(request.contains("criteria"));
    }

    #[tokio::test]
    async fn rejects_unknown_answers_and_off_mode_never_connects() {
        let (base, server) = fixture(200, r#"{"model":"nimble","answers":{"injected":{"type":"choice","choice":"support"},"codegg_decision_v1":{"type":"choice","choice":"support"}}}"#).await;
        let engine = SystemOneEngine::new(config(base, true));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("decision result");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
        let _ = server.await.expect("server completion");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind offline probe");
        let address = listener.local_addr().expect("probe address");
        let accept = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_ok()
        });
        let engine = SystemOneEngine::new(config(format!("http://{address}/v1"), false));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("off decision");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
        assert!(!accept.await.expect("accept probe completion"));
    }

    #[tokio::test]
    async fn deadline_timeout_returns_bounded_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind slow server");
        let address = listener.local_addr().expect("slow server address");
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("accept request");
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let mut cfg = config(format!("http://{address}/v1"), true);
        cfg.timeout = Duration::from_millis(30);
        let engine = SystemOneEngine::new(cfg);
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_millis(80))
            .await
            .expect("timeout fallback");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
        server.await.expect("slow server completion");
    }

    #[tokio::test]
    async fn http_and_protocol_failures_become_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind refused endpoint");
        let address = listener.local_addr().expect("refused address");
        drop(listener);
        let engine = SystemOneEngine::new(config(format!("http://{address}/v1"), true));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("connection fallback");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));

        for status in [401, 403, 404, 422, 429, 500, 503] {
            let (base, server) = fixture(status, r#"{"error":"redacted"}"#).await;
            let engine = SystemOneEngine::new(config(base, true));
            let result = engine
                .decide(choice_request(), Instant::now() + Duration::from_secs(3))
                .await
                .expect("bounded unavailable response");
            assert!(
                matches!(result.status, DecisionStatus::Unavailable { .. }),
                "status {status}"
            );
            let _ = server.await.expect("server completion");
        }
        let destination = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect destination");
        let destination_url = format!(
            "http://{}/redirected",
            destination.local_addr().expect("redirect target")
        );
        let forwarded = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_millis(150), destination.accept())
                .await
                .is_ok()
        });
        let (base, server) = fixture_with_location(302, Some(destination_url), "").await;
        let engine = SystemOneEngine::new(config(base, true));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("redirect fallback");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
        let _ = server.await.expect("redirect source completion");
        assert!(!forwarded.await.expect("redirect spy completion"));

        for body in [
            "{",
            r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"choice","choice":"not-requested"}}}"#,
            r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"noul","noul":0.5}}}"#,
            r#"{"model":"nimble","answers":{"codegg_decision_v1":{"type":"choice","choice":"support","probabilities":{"support":2.0}}}}"#,
        ] {
            let (base, server) = fixture(200, body).await;
            let engine = SystemOneEngine::new(config(base, true));
            let result = engine
                .decide(choice_request(), Instant::now() + Duration::from_secs(3))
                .await
                .expect("bounded protocol failure");
            assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
            let _ = server.await.expect("server completion");
        }
        let oversized =
            Box::leak(format!("{{\"padding\":\"{}\"}}", "x".repeat(MAX_BODY + 1)).into_boxed_str());
        let (base, server) = fixture(200, oversized).await;
        let engine = SystemOneEngine::new(config(base, true));
        let result = engine
            .decide(choice_request(), Instant::now() + Duration::from_secs(3))
            .await
            .expect("bounded oversized response");
        assert!(matches!(result.status, DecisionStatus::Unavailable { .. }));
        let _ = server.await.expect("oversized server completion");
    }

    #[tokio::test]
    async fn rank_is_unsupported_and_discovery_is_explicit() {
        let engine = SystemOneEngine::new(config("http://127.0.0.1:11434/v1".into(), true));
        let mut request = choice_request();
        request.spec = DecisionSpec::Rank {
            instruction: "rank candidates".into(),
            candidates: vec![codegg_core::decision::DecisionCandidate {
                id: "one".into(),
                label: "One".into(),
                description: "candidate".into(),
            }],
            multi_relevance: true,
        };
        let result = engine
            .decide(request, Instant::now() + Duration::from_secs(2))
            .await
            .expect("unsupported rank");
        assert!(matches!(result.status, DecisionStatus::Unsupported { .. }));

        let (base, server) = fixture(200, r#"{"models":[{"name":"nimble"},{"id":"tev"}]}"#).await;
        let mut discover_config = config(base, true);
        discover_config.discover_models = true;
        let engine = SystemOneEngine::new(discover_config);
        assert_eq!(
            engine.discover_models().await.expect("discovery"),
            ["nimble", "tev"]
        );
        let request = server.await.expect("discovery request");
        assert!(request.starts_with("GET /v1/models "));
    }

    #[test]
    fn privacy_diagnostics_include_only_field_names_and_sizes() {
        let mut request = choice_request();
        request.state[0].value = "private-content-sentinel".into();
        let diagnostic = privacy_diagnostic(&request).expect("diagnostic");
        let rendered = format!("{diagnostic:?}");
        assert!(rendered.contains("kind"));
        assert!(rendered.contains("value_bytes"));
        assert!(!rendered.contains("private-content-sentinel"));

        let credential = codegg_providers::Credential::api_key("private-credential-sentinel");
        assert!(!format!("{credential:?}").contains("private-credential-sentinel"));
    }
}
