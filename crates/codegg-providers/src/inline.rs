//! Inline provider descriptor: a per-session connection supplied by a host.
//!
//! The descriptor is never persisted. Its credential is resolved only when
//! `register_inline_provider` builds the provider, and neither `Debug` nor
//! `redacted_json` renders a credential or a header value.

use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider, ToolChoice};
use crate::provider_core::{ModelInfo, ProviderRegistry};
use http::header::HeaderName;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

pub const CHAT_COMPLETIONS_PROTOCOL: &str = "chat_completions";
pub const MAX_PROVIDER_ID_LEN: usize = 64;
pub const MAX_DISPLAY_NAME_LEN: usize = 64;
pub const MAX_PROVIDER_HEADERS: usize = 16;
pub const MAX_PROVIDER_HEADER_BYTES: usize = 8 * 1024;
pub const MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_CREDENTIAL_BYTES: usize = 8 * 1024;
pub const MAX_ENV_NAME_LEN: usize = 128;
pub const MAX_INLINE_MODELS: usize = 64;
pub const MAX_MODEL_ID_LEN: usize = 128;
pub const MAX_CONTEXT_WINDOW: usize = 16_000_000;
pub const MAX_OUTPUT_TOKENS: usize = 4_000_000;
pub const REDACTED: &str = "<redacted>";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderHeaders(Vec<ProviderHeader>);

impl ProviderHeaders {
    pub fn new(headers: Vec<ProviderHeader>) -> Result<Self, InlineProviderError> {
        let headers = Self(headers);
        headers.validate()?;
        Ok(headers)
    }

    pub fn validate(&self) -> Result<(), InlineProviderError> {
        if self.0.len() > MAX_PROVIDER_HEADERS {
            return Err(InlineProviderError::HeadersTooMany {
                max: MAX_PROVIDER_HEADERS,
            });
        }
        let mut total = 0usize;
        let mut seen = Vec::with_capacity(self.0.len());
        for (index, header) in self.0.iter().enumerate() {
            if header.value.len() > MAX_HEADER_VALUE_BYTES {
                return Err(InlineProviderError::HeaderValueTooLong {
                    index,
                    max: MAX_HEADER_VALUE_BYTES,
                });
            }
            if header
                .value
                .bytes()
                .any(|byte| byte == b'\r' || byte == b'\n')
            {
                return Err(InlineProviderError::HeaderValueContainsLineBreak { index });
            }
            if header
                .value
                .bytes()
                .any(|byte| byte != b'\t' && !(0x20..=0x7e).contains(&byte))
            {
                return Err(InlineProviderError::HeaderValueInvalid { index });
            }
            let name = HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| InlineProviderError::HeaderNameInvalid { index })?;
            if is_reserved_header(&name) {
                return Err(InlineProviderError::HeaderNameReserved { index });
            }
            if seen.contains(&name) {
                return Err(InlineProviderError::HeaderNameDuplicate { index });
            }
            seen.push(name);
            total = total
                .saturating_add(header.name.len())
                .saturating_add(header.value.len());
        }
        if total > MAX_PROVIDER_HEADER_BYTES {
            return Err(InlineProviderError::HeadersTooLarge {
                max: MAX_PROVIDER_HEADER_BYTES,
            });
        }
        Ok(())
    }

    pub fn as_slice(&self) -> &[ProviderHeader] {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ProviderHeader> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for ProviderHeaders {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = formatter.debug_list();
        for header in &self.0 {
            list.entry(&std::collections::BTreeMap::from([
                ("name", header.name.as_str()),
                ("value", REDACTED),
            ]));
        }
        list.finish()
    }
}

fn is_reserved_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "authorization" | "proxy-authorization" | "content-type" | "user-agent"
    )
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineCredential {
    ApiKey { value: String },
    Env { name: String },
}

impl InlineCredential {
    /// Resolve the credential material at registration time. The `env` form
    /// reads the variable here and never retains its name as a secret.
    pub fn resolve(&self) -> Result<Credential, InlineProviderError> {
        match self {
            Self::ApiKey { value } => {
                validate_api_key(value)?;
                Ok(Credential::api_key(value.clone()))
            }
            Self::Env { name } => {
                let value =
                    std::env::var(name).map_err(|_| InlineProviderError::CredentialEnvNotSet)?;
                validate_api_key(&value)?;
                Ok(Credential::api_key(value))
            }
        }
    }

    pub fn redacted_json(&self) -> serde_json::Value {
        match self {
            Self::ApiKey { .. } => serde_json::json!({
                "kind": "api_key",
                "value": REDACTED,
            }),
            Self::Env { name } => serde_json::json!({
                "kind": "env",
                "name": name,
            }),
        }
    }
}

impl std::fmt::Debug for InlineCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey { .. } => formatter
                .debug_struct("InlineCredential::ApiKey")
                .field("value", &REDACTED)
                .finish(),
            Self::Env { name } => formatter
                .debug_struct("InlineCredential::Env")
                .field("name", name)
                .finish(),
        }
    }
}

fn validate_api_key(value: &str) -> Result<(), InlineProviderError> {
    if value.is_empty() {
        return Err(InlineProviderError::CredentialEmpty);
    }
    if value.len() > MAX_CREDENTIAL_BYTES {
        return Err(InlineProviderError::CredentialTooLong {
            max: MAX_CREDENTIAL_BYTES,
        });
    }
    if value.chars().any(char::is_control) {
        return Err(InlineProviderError::CredentialInvalid);
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), InlineProviderError> {
    if name.is_empty() {
        return Err(InlineProviderError::CredentialEnvEmpty);
    }
    if name.len() > MAX_ENV_NAME_LEN {
        return Err(InlineProviderError::CredentialEnvTooLong {
            max: MAX_ENV_NAME_LEN,
        });
    }
    let mut characters = name.chars();
    let first_ok = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    if !first_ok
        || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        return Err(InlineProviderError::CredentialEnvInvalid);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineProviderDescriptor {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub headers: ProviderHeaders,
    pub credential: InlineCredential,
    pub protocol: String,
    #[serde(default)]
    pub models: Vec<InlineModel>,
}

impl InlineProviderDescriptor {
    pub fn validate(&self) -> Result<(), InlineProviderError> {
        validate_id(&self.id)?;
        if let Some(display_name) = &self.display_name {
            if display_name.chars().count() > MAX_DISPLAY_NAME_LEN {
                return Err(InlineProviderError::DisplayNameTooLong {
                    max: MAX_DISPLAY_NAME_LEN,
                });
            }
            if display_name.chars().any(char::is_control) {
                return Err(InlineProviderError::DisplayNameInvalid);
            }
        }
        validate_base_url(&self.base_url)?;
        self.headers.validate()?;
        match &self.credential {
            InlineCredential::ApiKey { value } => validate_api_key(value)?,
            InlineCredential::Env { name } => validate_env_name(name)?,
        }
        if self.protocol != CHAT_COMPLETIONS_PROTOCOL {
            return Err(InlineProviderError::UnsupportedProtocol);
        }
        if self.models.len() > MAX_INLINE_MODELS {
            return Err(InlineProviderError::ModelsTooMany {
                max: MAX_INLINE_MODELS,
            });
        }
        let mut seen = Vec::with_capacity(self.models.len());
        for (index, model) in self.models.iter().enumerate() {
            validate_inline_model(index, model)?;
            if seen.contains(&model.id) {
                return Err(InlineProviderError::ModelIdDuplicate { index });
            }
            seen.push(model.id.clone());
        }
        Ok(())
    }

    /// The descriptor with every secret-bearing value replaced. Safe to log;
    /// the wire form still carries the real values.
    pub fn redacted_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": &self.id,
            "displayName": &self.display_name,
            "baseUrl": &self.base_url,
            "protocol": &self.protocol,
            "headers": self
                .headers
                .iter()
                .map(|header| serde_json::json!({
                    "name": &header.name,
                    "value": REDACTED,
                }))
                .collect::<Vec<_>>(),
            "credential": self.credential.redacted_json(),
            "models": &self.models,
        })
    }
}

impl std::fmt::Debug for InlineProviderDescriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InlineProviderDescriptor")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("base_url", &self.base_url)
            .field("headers", &self.headers)
            .field("credential", &self.credential)
            .field("protocol", &self.protocol)
            .field("models", &self.models)
            .finish()
    }
}

fn validate_id(id: &str) -> Result<(), InlineProviderError> {
    if id.is_empty() {
        return Err(InlineProviderError::IdEmpty);
    }
    if id.len() > MAX_PROVIDER_ID_LEN {
        return Err(InlineProviderError::IdTooLong {
            max: MAX_PROVIDER_ID_LEN,
        });
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
    {
        return Err(InlineProviderError::IdInvalidCharacter);
    }
    Ok(())
}

fn validate_base_url(base_url: &str) -> Result<(), InlineProviderError> {
    if base_url.chars().any(char::is_control) {
        return Err(InlineProviderError::BaseUrlInvalid);
    }
    let url = Url::parse(base_url).map_err(|_| InlineProviderError::BaseUrlInvalid)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(InlineProviderError::BaseUrlScheme);
    }
    if url.host_str().is_none() {
        return Err(InlineProviderError::BaseUrlMissingHost);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(InlineProviderError::BaseUrlContainsCredentials);
    }
    if url.port() == Some(0) || url.query().is_some() || url.fragment().is_some() {
        return Err(InlineProviderError::BaseUrlInvalid);
    }
    if url
        .path_segments()
        .is_some_and(|mut segments| segments.any(|segment| segment == ".."))
    {
        return Err(InlineProviderError::BaseUrlInvalid);
    }
    Ok(())
}

fn validate_inline_model(index: usize, model: &InlineModel) -> Result<(), InlineProviderError> {
    if model.id.is_empty() {
        return Err(InlineProviderError::ModelIdEmpty { index });
    }
    if model.id.len() > MAX_MODEL_ID_LEN {
        return Err(InlineProviderError::ModelIdTooLong {
            index,
            max: MAX_MODEL_ID_LEN,
        });
    }
    if model.id.chars().any(char::is_control) {
        return Err(InlineProviderError::ModelIdInvalid { index });
    }
    if let Some(context_window) = model.context_window {
        if context_window == 0 {
            return Err(InlineProviderError::ContextWindowZero { index });
        }
        if context_window > MAX_CONTEXT_WINDOW {
            return Err(InlineProviderError::ContextWindowTooLarge {
                index,
                max: MAX_CONTEXT_WINDOW,
            });
        }
    }
    if let Some(max_output_tokens) = model.max_output_tokens {
        if max_output_tokens == 0 {
            return Err(InlineProviderError::MaxOutputTokensZero { index });
        }
        if max_output_tokens > MAX_OUTPUT_TOKENS {
            return Err(InlineProviderError::MaxOutputTokensTooLarge {
                index,
                max: MAX_OUTPUT_TOKENS,
            });
        }
        if model
            .context_window
            .is_some_and(|context_window| max_output_tokens > context_window)
        {
            return Err(InlineProviderError::MaxOutputExceedsContext { index });
        }
    }
    Ok(())
}

/// Build a ready provider from `descriptor` and register it under
/// `descriptor.id`. Config, the credential store and the reviewed wire map
/// are not consulted.
pub fn register_inline_provider(
    registry: &mut ProviderRegistry,
    descriptor: &InlineProviderDescriptor,
) -> Result<(), InlineProviderError> {
    descriptor.validate()?;
    let base_url = crate::eggpool::normalize_compatible_base_url(&descriptor.base_url)
        .map_err(|_| InlineProviderError::BaseUrlInvalid)?;
    let credential = descriptor.credential.resolve()?;
    let models = descriptor
        .models
        .iter()
        .map(|model| ModelInfo {
            id: model.id.clone(),
            name: model.id.clone(),
            provider: descriptor.id.clone(),
            context_window: model.context_window.unwrap_or(0),
            max_output_tokens: model.max_output_tokens,
            supports_tools: model.supports_tools.unwrap_or(true),
            supports_vision: false,
            variants: Vec::new(),
        })
        .collect();
    let provider = OpenAiCompatibleProvider::new(
        &descriptor.id,
        descriptor.display_name.as_deref().unwrap_or(&descriptor.id),
        OpenAiCompatibleConfig {
            credential,
            base_url,
            auth_header: "Authorization".to_string(),
            extra_headers: Vec::new(),
            models,
            tool_choice: ToolChoice::Auto,
        },
    )
    .with_provider_headers(descriptor.headers.clone())?;
    registry.register(provider);
    Ok(())
}

#[derive(Debug, Error)]
pub enum InlineProviderError {
    #[error("descriptor field `id` is empty")]
    IdEmpty,
    #[error("descriptor field `id` exceeds {max} characters")]
    IdTooLong { max: usize },
    #[error("descriptor field `id` contains a character outside [a-z0-9._-]")]
    IdInvalidCharacter,
    #[error("descriptor field `displayName` exceeds {max} characters")]
    DisplayNameTooLong { max: usize },
    #[error("descriptor field `displayName` contains a control character")]
    DisplayNameInvalid,
    #[error("descriptor field `baseUrl` is not an absolute URL")]
    BaseUrlInvalid,
    #[error("descriptor field `baseUrl` scheme is not http or https")]
    BaseUrlScheme,
    #[error("descriptor field `baseUrl` has no host")]
    BaseUrlMissingHost,
    #[error("descriptor field `baseUrl` must not carry credentials")]
    BaseUrlContainsCredentials,
    #[error("descriptor field `headers` exceeds {max} entries")]
    HeadersTooMany { max: usize },
    #[error("descriptor field `headers` exceeds {max} bytes")]
    HeadersTooLarge { max: usize },
    #[error("descriptor field `headers[{index}].name` is not a valid HTTP header name")]
    HeaderNameInvalid { index: usize },
    #[error("descriptor field `headers[{index}].name` is reserved by the transport")]
    HeaderNameReserved { index: usize },
    #[error("descriptor field `headers[{index}].name` is declared more than once")]
    HeaderNameDuplicate { index: usize },
    #[error("descriptor field `headers[{index}].value` exceeds {max} bytes")]
    HeaderValueTooLong { index: usize, max: usize },
    #[error("descriptor field `headers[{index}].value` contains a line break")]
    HeaderValueContainsLineBreak { index: usize },
    #[error("descriptor field `headers[{index}].value` contains a control character")]
    HeaderValueInvalid { index: usize },
    #[error("descriptor field `credential.value` is empty")]
    CredentialEmpty,
    #[error("descriptor field `credential.value` exceeds {max} bytes")]
    CredentialTooLong { max: usize },
    #[error("descriptor field `credential.value` contains a control character")]
    CredentialInvalid,
    #[error("descriptor field `credential.name` is empty")]
    CredentialEnvEmpty,
    #[error("descriptor field `credential.name` exceeds {max} characters")]
    CredentialEnvTooLong { max: usize },
    #[error("descriptor field `credential.name` is not a valid environment variable name")]
    CredentialEnvInvalid,
    #[error("descriptor field `credential.name` names an unset environment variable")]
    CredentialEnvNotSet,
    #[error("descriptor field `protocol` declares an unsupported wire surface")]
    UnsupportedProtocol,
    #[error("descriptor field `models` exceeds {max} entries")]
    ModelsTooMany { max: usize },
    #[error("descriptor field `models[{index}].id` is empty")]
    ModelIdEmpty { index: usize },
    #[error("descriptor field `models[{index}].id` exceeds {max} characters")]
    ModelIdTooLong { index: usize, max: usize },
    #[error("descriptor field `models[{index}].id` contains a control character")]
    ModelIdInvalid { index: usize },
    #[error("descriptor field `models[{index}].id` is declared more than once")]
    ModelIdDuplicate { index: usize },
    #[error("descriptor field `models[{index}].contextWindow` must be greater than zero")]
    ContextWindowZero { index: usize },
    #[error("descriptor field `models[{index}].contextWindow` exceeds {max}")]
    ContextWindowTooLarge { index: usize, max: usize },
    #[error("descriptor field `models[{index}].maxOutputTokens` must be greater than zero")]
    MaxOutputTokensZero { index: usize },
    #[error("descriptor field `models[{index}].maxOutputTokens` exceeds {max}")]
    MaxOutputTokensTooLarge { index: usize, max: usize },
    #[error("descriptor field `models[{index}].maxOutputTokens` exceeds its contextWindow")]
    MaxOutputExceedsContext { index: usize },
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_core::{ChatRequest, ContentPart, Message, ProviderRequestContext};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;

    const DUMMY_SECRET: &str = "sk-test-dummy-never-real";
    const DUMMY_HEADER_VALUE: &str = "dummy-header-value";

    fn headers(pairs: &[(&str, &str)]) -> ProviderHeaders {
        ProviderHeaders::new(
            pairs
                .iter()
                .map(|(name, value)| ProviderHeader {
                    name: (*name).to_string(),
                    value: (*value).to_string(),
                })
                .collect(),
        )
        .expect("test headers are valid")
    }

    fn descriptor(base_url: &str) -> InlineProviderDescriptor {
        InlineProviderDescriptor {
            id: "inline-test".to_string(),
            display_name: Some("Inline Test".to_string()),
            base_url: base_url.to_string(),
            headers: headers(&[("x-tenant", DUMMY_HEADER_VALUE)]),
            credential: InlineCredential::ApiKey {
                value: DUMMY_SECRET.to_string(),
            },
            protocol: CHAT_COMPLETIONS_PROTOCOL.to_string(),
            models: vec![InlineModel {
                id: "outside-builtin-list".to_string(),
                context_window: Some(64_000),
                max_output_tokens: Some(4_096),
                supports_tools: Some(true),
            }],
        }
    }

    #[test]
    fn descriptor_validation_matrix_names_each_rejected_field() {
        let valid = descriptor("https://example.invalid/v1");
        assert!(valid.validate().is_ok());

        let mut bad = valid.clone();
        bad.base_url = "ftp://example.invalid/v1".to_string();
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::BaseUrlScheme)
        ));

        let mut bad = valid.clone();
        bad.base_url = "http://".to_string();
        let error = bad.validate().expect_err("a host is required");
        assert!(matches!(error, InlineProviderError::BaseUrlInvalid));
        assert!(error.to_string().contains("baseUrl"));

        let mut bad = valid.clone();
        bad.base_url = "https://user:pass@example.invalid/v1".to_string();
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::BaseUrlContainsCredentials)
        ));

        let mut bad = valid.clone();
        bad.id = String::new();
        assert!(matches!(bad.validate(), Err(InlineProviderError::IdEmpty)));

        let mut bad = valid.clone();
        bad.id = "a".repeat(MAX_PROVIDER_ID_LEN + 1);
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::IdTooLong { .. })
        ));

        let mut bad = valid.clone();
        bad.id = "Upper".to_string();
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::IdInvalidCharacter)
        ));

        let mut bad = valid.clone();
        bad.display_name = Some("d".repeat(MAX_DISPLAY_NAME_LEN + 1));
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::DisplayNameTooLong { .. })
        ));

        let mut bad = valid.clone();
        bad.protocol = "responses".to_string();
        let error = bad.validate().expect_err("unknown protocol is refused");
        assert!(matches!(error, InlineProviderError::UnsupportedProtocol));
        assert!(error.to_string().contains("protocol"));
        assert!(!error.to_string().contains(DUMMY_SECRET));

        let mut bad = valid.clone();
        bad.models = vec![InlineModel {
            id: "m".to_string(),
            context_window: Some(1),
            max_output_tokens: Some(2),
            supports_tools: None,
        }];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::MaxOutputExceedsContext { .. })
        ));

        let mut bad = valid.clone();
        bad.models = vec![InlineModel {
            id: "m".to_string(),
            context_window: Some(0),
            max_output_tokens: None,
            supports_tools: None,
        }];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::ContextWindowZero { .. })
        ));

        let mut bad = valid.clone();
        bad.models = vec![InlineModel {
            id: "m".to_string(),
            context_window: None,
            max_output_tokens: Some(0),
            supports_tools: None,
        }];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::MaxOutputTokensZero { .. })
        ));

        let mut bad = valid.clone();
        bad.models = vec![InlineModel {
            id: "m".to_string(),
            context_window: Some(MAX_CONTEXT_WINDOW + 1),
            max_output_tokens: None,
            supports_tools: None,
        }];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::ContextWindowTooLarge { .. })
        ));

        let mut bad = valid.clone();
        bad.models = vec![InlineModel {
            id: "m".repeat(MAX_MODEL_ID_LEN + 1),
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
        }];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::ModelIdTooLong { .. })
        ));

        let mut bad = valid.clone();
        bad.models = (0..=MAX_INLINE_MODELS)
            .map(|index| InlineModel {
                id: format!("model-{index}"),
                context_window: None,
                max_output_tokens: None,
                supports_tools: None,
            })
            .collect();
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::ModelsTooMany { .. })
        ));

        let mut bad = valid.clone();
        bad.models = vec![
            InlineModel {
                id: "same".to_string(),
                context_window: None,
                max_output_tokens: None,
                supports_tools: None,
            },
            InlineModel {
                id: "same".to_string(),
                context_window: None,
                max_output_tokens: None,
                supports_tools: None,
            },
        ];
        assert!(matches!(
            bad.validate(),
            Err(InlineProviderError::ModelIdDuplicate { .. })
        ));
    }

    #[test]
    fn header_validation_matrix_names_fields_without_values() {
        let cases = [
            (
                vec![ProviderHeader {
                    name: "bad name".to_string(),
                    value: DUMMY_HEADER_VALUE.to_string(),
                }],
                "headers[0].name",
            ),
            (
                vec![ProviderHeader {
                    name: "x-test".to_string(),
                    value: "x".repeat(MAX_HEADER_VALUE_BYTES + 1),
                }],
                "headers[0].value",
            ),
            (
                vec![
                    ProviderHeader {
                        name: "x-test".to_string(),
                        value: "one".to_string(),
                    },
                    ProviderHeader {
                        name: "X-Test".to_string(),
                        value: "two".to_string(),
                    },
                ],
                "headers[1].name",
            ),
            (
                vec![ProviderHeader {
                    name: "Authorization".to_string(),
                    value: DUMMY_HEADER_VALUE.to_string(),
                }],
                "headers[0].name",
            ),
            (
                vec![ProviderHeader {
                    name: "Content-Type".to_string(),
                    value: DUMMY_HEADER_VALUE.to_string(),
                }],
                "headers[0].name",
            ),
            (
                vec![ProviderHeader {
                    name: "x-test".to_string(),
                    value: "bad\r\nvalue".to_string(),
                }],
                "headers[0].value",
            ),
        ];
        for (input, field) in cases {
            let error = ProviderHeaders::new(input).expect_err("invalid headers are refused");
            let text = error.to_string();
            assert!(text.contains(field), "error `{text}` must name {field}");
            assert!(!text.contains(DUMMY_HEADER_VALUE));
            assert!(!text.contains("bad\r\nvalue"));
        }

        let repeated = (0..=MAX_PROVIDER_HEADERS)
            .map(|index| ProviderHeader {
                name: format!("x-{index}"),
                value: "v".to_string(),
            })
            .collect();
        assert!(matches!(
            ProviderHeaders::new(repeated),
            Err(InlineProviderError::HeadersTooMany { .. })
        ));
    }

    #[test]
    fn descriptor_wire_form_is_camel_case_and_round_trips() {
        let original = descriptor("https://example.invalid/v1");
        let wire = serde_json::to_value(&original).expect("serialize descriptor");
        assert_eq!(wire["baseUrl"], "https://example.invalid/v1");
        assert_eq!(wire["displayName"], "Inline Test");
        assert_eq!(wire["credential"]["kind"], "api_key");
        assert_eq!(wire["credential"]["value"], DUMMY_SECRET);
        assert_eq!(wire["headers"][0]["name"], "x-tenant");
        assert_eq!(wire["models"][0]["contextWindow"], 64_000);
        assert_eq!(wire["models"][0]["maxOutputTokens"], 4_096);
        assert_eq!(wire["models"][0]["supportsTools"], true);
        assert!(wire.get("base_url").is_none());

        let decoded: InlineProviderDescriptor =
            serde_json::from_value(wire).expect("deserialize descriptor");
        assert_eq!(decoded, original);

        let env_wire = json!({
            "id": "env-provider",
            "baseUrl": "https://example.invalid/v1",
            "protocol": CHAT_COMPLETIONS_PROTOCOL,
            "credential": {"kind": "env", "name": "CODEGG_E001_TEST_KEY"}
        });
        let env_decoded: InlineProviderDescriptor =
            serde_json::from_value(env_wire).expect("env descriptor");
        assert!(env_decoded.headers.is_empty());
        assert!(env_decoded.models.is_empty());
    }

    #[test]
    fn debug_and_redacted_json_never_render_secrets() {
        let original = descriptor("https://example.invalid/v1");
        let debug = format!("{:?}", original);
        assert!(!debug.contains(DUMMY_SECRET));
        assert!(!debug.contains(DUMMY_HEADER_VALUE));
        assert!(debug.contains(REDACTED));

        let redacted = original.redacted_json().to_string();
        assert!(!redacted.contains(DUMMY_SECRET));
        assert!(!redacted.contains(DUMMY_HEADER_VALUE));
        assert!(redacted.contains("x-tenant"));
        assert!(redacted.contains(REDACTED));
    }

    #[test]
    fn validation_error_display_never_renders_the_credential() {
        let mut invalid = descriptor("https://example.invalid/v1");
        invalid.id = "Bad".to_string();
        let error = invalid.validate().expect_err("invalid id");
        assert!(error.to_string().contains("id"));
        assert!(!error.to_string().contains(DUMMY_SECRET));

        let mut invalid = descriptor("https://example.invalid/v1");
        invalid.protocol = "messages".to_string();
        let error = invalid.validate().expect_err("unsupported protocol");
        assert!(!error.to_string().contains(DUMMY_SECRET));
    }

    #[test]
    fn log_capture_never_renders_secrets() {
        struct Capture(Arc<Mutex<String>>);
        struct Collect<'a>(&'a mut String);

        impl tracing::field::Visit for Collect<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{}={:?};", field.name(), value));
            }
        }

        impl tracing::Subscriber for Capture {
            fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {
            }
            fn enter(&self, _span: &tracing::span::Id) {}
            fn exit(&self, _span: &tracing::span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                let mut text = String::new();
                event.record(&mut Collect(&mut text));
                self.0.lock().expect("log capture lock").push_str(&text);
            }
        }

        let log = Arc::new(Mutex::new(String::new()));
        tracing::subscriber::with_default(Capture(Arc::clone(&log)), || {
            tracing::info!(
                descriptor = ?descriptor("https://example.invalid/v1"),
                "inline descriptor"
            );
        });
        let captured = log.lock().expect("log capture lock");
        assert!(!captured.contains(DUMMY_SECRET));
        assert!(!captured.contains(DUMMY_HEADER_VALUE));
        assert!(captured.contains(REDACTED));
    }

    #[test]
    fn crlf_headers_are_refused_by_validation_before_io() {
        let wire = json!({
            "id": "crlf-provider",
            "baseUrl": "https://example.invalid/v1",
            "protocol": CHAT_COMPLETIONS_PROTOCOL,
            "headers": [{"name": "x-test", "value": "bad\r\nvalue"}],
            "credential": {"kind": "api_key", "value": DUMMY_SECRET}
        });
        let descriptor: InlineProviderDescriptor =
            serde_json::from_value(wire).expect("wire shape is deserializable");
        let error = descriptor
            .validate()
            .expect_err("CR/LF header value is refused");
        assert!(error.to_string().contains("headers[0].value"));
        assert!(!error.to_string().contains(DUMMY_SECRET));
    }

    #[test]
    fn provider_id_is_absent_without_a_descriptor() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("inline-test").is_none());
    }

    #[test]
    fn two_registries_hold_the_same_descriptor_id_independently() {
        let inline = descriptor("https://example.invalid/v1");
        let mut first = ProviderRegistry::new();
        let mut second = ProviderRegistry::new();
        register_inline_provider(&mut first, &inline).expect("register first");
        register_inline_provider(&mut second, &inline).expect("register second");
        assert!(first.get("inline-test").is_some());
        assert!(second.get("inline-test").is_some());
        let third = ProviderRegistry::new();
        assert!(third.get("inline-test").is_none());
    }

    #[test]
    fn env_credential_resolves_at_registration_and_stays_redacted() {
        const ENV_NAME: &str = "CODEGG_E001_INLINE_TEST_KEY";
        std::env::set_var(ENV_NAME, DUMMY_SECRET);
        let mut inline = descriptor("https://example.invalid/v1");
        inline.credential = InlineCredential::Env {
            name: ENV_NAME.to_string(),
        };

        let credential = inline
            .credential
            .resolve()
            .expect("env credential resolves");
        assert_eq!(credential.secret, DUMMY_SECRET);
        assert!(!format!("{:?}", inline.credential).contains(DUMMY_SECRET));
        assert!(!inline.redacted_json().to_string().contains(DUMMY_SECRET));

        let mut registry = ProviderRegistry::new();
        register_inline_provider(&mut registry, &inline).expect("register env credential");
        assert!(registry.get("inline-test").is_some());
        std::env::remove_var(ENV_NAME);
    }

    #[test]
    fn missing_env_credential_is_refused_without_registering() {
        let mut inline = descriptor("https://example.invalid/v1");
        inline.credential = InlineCredential::Env {
            name: "CODEGG_E001_UNSET_TEST_KEY".to_string(),
        };
        let mut registry = ProviderRegistry::new();
        let error = register_inline_provider(&mut registry, &inline)
            .expect_err("unset env credential is refused");
        assert!(matches!(error, InlineProviderError::CredentialEnvNotSet));
        assert!(registry.get("inline-test").is_none());
    }

    #[test]
    fn unknown_protocol_is_refused_without_registering() {
        let mut inline = descriptor("https://example.invalid/v1");
        inline.protocol = "responses".to_string();
        let mut registry = ProviderRegistry::new();
        let error = register_inline_provider(&mut registry, &inline)
            .expect_err("unknown protocol is refused");
        assert!(matches!(error, InlineProviderError::UnsupportedProtocol));
        assert!(registry.get("inline-test").is_none());
    }

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

    fn header_values<'a>(request: &'a CapturedRequest, name: &str) -> Vec<&'a str> {
        request
            .headers
            .iter()
            .filter(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn chat_request(model: &str) -> ChatRequest {
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
            context: ProviderRequestContext::default(),
        }
    }

    #[tokio::test]
    async fn registered_descriptor_drives_one_request_with_declared_headers() {
        let (base_url, rx, server) = spawn_capture_server(1);
        let mut inline = descriptor(&base_url);
        inline.headers = headers(&[("x-tenant", DUMMY_HEADER_VALUE), ("x-routing-hint", "beta")]);
        let mut registry = ProviderRegistry::new();
        register_inline_provider(&mut registry, &inline).expect("register descriptor");
        let provider = registry
            .get("inline-test")
            .expect("descriptor provider registered");

        let _stream = provider
            .stream(&chat_request("outside-builtin-list"))
            .await
            .expect("stream through capture server");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");

        assert_eq!(
            header_values(&captured, "x-tenant"),
            vec![DUMMY_HEADER_VALUE]
        );
        assert_eq!(header_values(&captured, "x-routing-hint"), vec!["beta"]);
        assert_eq!(
            header_values(&captured, "authorization"),
            vec!["Bearer sk-test-dummy-never-real"]
        );
        assert!(captured.body.contains("\"model\":\"outside-builtin-list\""));
    }
}
