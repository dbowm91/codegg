//! Secret-safe provider connection descriptors and lazy construction.
//!
//! A connection descriptor contains provider metadata and an opaque reference
//! to a record in [`CredentialStore`].  It deliberately never contains a
//! resolved secret.  The existing provider registration functions continue to
//! own environment/config compatibility; this module is the daemon-facing
//! seam for persisted connections.

use crate::anthropic::AnthropicProvider;
use crate::auth_types::{Credential, CredentialKind, CredentialStore, StoredCredentialRecord};
use crate::openai::{OpenAiConfig, OpenAiProvider};
use crate::openai_compatible::OpenAiCompatibleProvider;
use crate::Provider;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use thiserror::Error;
use url::Url;

/// An opaque reference to an existing encrypted credential-store record.
///
/// `account_id` is matched exactly.  In particular, resolution never falls
/// back from one account to another account or to a provider-wide record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SecretRef {
    pub provider_id: String,
    #[serde(default)]
    pub account_id: Option<String>,
}

impl SecretRef {
    pub fn new(provider_id: impl Into<String>, account_id: Option<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            account_id,
        }
    }

    pub fn provider(provider_id: impl Into<String>) -> Self {
        Self::new(provider_id, None::<String>)
    }
}

/// Provider implementation requested by a persisted connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
    Google,
    #[serde(rename = "azure_openai")]
    AzureOpenAi,
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible {
        provider_id: String,
    },
}

impl ProviderKind {
    fn implementation_id(&self) -> &str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::AzureOpenAi => "azure",
            Self::OpenAiCompatible { provider_id } => provider_id,
        }
    }

    fn default_name(&self) -> &str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Google => "Google",
            Self::AzureOpenAi => "Azure OpenAI",
            Self::OpenAiCompatible { provider_id } => provider_id,
        }
    }
}

/// Secret-free metadata persisted by the daemon for one provider connection.
///
/// `connection_id` identifies the configured connection and is intentionally
/// distinct from the provider implementation ID used by [`Provider::id`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConnectionDescriptor {
    pub connection_id: String,
    pub provider: ProviderKind,
    pub secret_ref: SecretRef,
    /// Optional provider endpoint.  Query strings and fragments are rejected
    /// because endpoint metadata must not become a secret-bearing URL.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

impl ProviderConnectionDescriptor {
    pub fn new(
        connection_id: impl Into<String>,
        provider: ProviderKind,
        secret_ref: SecretRef,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            provider,
            secret_ref,
            base_url: None,
            display_name: None,
        }
    }

    pub fn openai(connection_id: impl Into<String>, secret_ref: SecretRef) -> Self {
        Self::new(connection_id, ProviderKind::OpenAi, secret_ref)
    }

    pub fn anthropic(connection_id: impl Into<String>, secret_ref: SecretRef) -> Self {
        Self::new(connection_id, ProviderKind::Anthropic, secret_ref)
    }

    pub fn openai_compatible(
        connection_id: impl Into<String>,
        provider_id: impl Into<String>,
        secret_ref: SecretRef,
        base_url: impl Into<String>,
    ) -> Self {
        let mut descriptor = Self::new(
            connection_id,
            ProviderKind::OpenAiCompatible {
                provider_id: provider_id.into(),
            },
            secret_ref,
        );
        descriptor.base_url = Some(base_url.into());
        descriptor
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn validate(&self) -> Result<(), ConnectionError> {
        if self.connection_id.trim().is_empty() {
            return Err(ConnectionError::InvalidDescriptor(
                "connection_id must not be empty".to_string(),
            ));
        }
        if self.secret_ref.provider_id.trim().is_empty() {
            return Err(ConnectionError::InvalidDescriptor(
                "secret_ref.provider_id must not be empty".to_string(),
            ));
        }
        if let ProviderKind::OpenAiCompatible { provider_id } = &self.provider {
            if provider_id.trim().is_empty() {
                return Err(ConnectionError::InvalidDescriptor(
                    "openai-compatible provider_id must not be empty".to_string(),
                ));
            }
            if self
                .base_url
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                return Err(ConnectionError::InvalidDescriptor(
                    "openai-compatible connections require base_url".to_string(),
                ));
            }
        }
        if let Some(base_url) = self.base_url.as_deref() {
            validate_base_url(base_url)?;
        }
        Ok(())
    }
}

/// Compatibility name for callers that use the shorter descriptor term.
pub type ConnectionDescriptor = ProviderConnectionDescriptor;
/// Compatibility name matching the domain terminology in the connection plan.
pub type ProviderConnection = ProviderConnectionDescriptor;
/// Compatibility name for provider-kind fields in older manager code.
pub type ConnectionKind = ProviderKind;
/// Compatibility name for secret-reference fields in older manager code.
pub type SecretReference = SecretRef;

fn validate_base_url(value: &str) -> Result<(), ConnectionError> {
    let url = Url::parse(value).map_err(|_| {
        ConnectionError::InvalidDescriptor("base_url must be a valid URL".to_string())
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ConnectionError::InvalidDescriptor(
            "base_url must use http or https and include a host".to_string(),
        ));
    }
    if url.username() != "" || url.password().is_some() || url.query().is_some() {
        return Err(ConnectionError::InvalidDescriptor(
            "base_url must not contain userinfo or a query string".to_string(),
        ));
    }
    if url.fragment().is_some() {
        return Err(ConnectionError::InvalidDescriptor(
            "base_url must not contain a fragment".to_string(),
        ));
    }
    Ok(())
}

/// Errors returned while resolving a [`SecretRef`].
#[derive(Debug, Error)]
pub enum SecretResolutionError {
    #[error("credential missing for provider '{provider_id}'{account}")]
    Missing {
        provider_id: String,
        account: AccountDescription,
    },
    #[error("credential expired for provider '{provider_id}'{account}")]
    Expired {
        provider_id: String,
        account: AccountDescription,
    },
    #[error("master key missing while resolving credential for provider '{provider_id}'")]
    MasterKeyMissing { provider_id: String },
    #[error("credential store error: {0}")]
    Store(#[source] crate::auth_types::AuthError),
}

/// Redacted account context used in secret-resolution diagnostics.
#[derive(Debug, Clone)]
pub struct AccountDescription(Option<String>);

impl fmt::Display for AccountDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.as_deref() {
            Some(account_id) => write!(f, " for account '{account_id}'"),
            None => Ok(()),
        }
    }
}

/// Source-independent credential lookup seam for a daemon connection manager.
pub trait SecretResolver: Send + Sync {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<Credential, SecretResolutionError>;
}

/// Resolves secret references through the existing encrypted credential store.
///
/// The adapter performs metadata checks before decrypting and requires a
/// configured master key for an existing record.  It does not expose the store
/// records or provide a provider/account fallback path.
#[derive(Clone)]
pub struct CredentialStoreAdapter {
    store: Arc<CredentialStore>,
}

impl CredentialStoreAdapter {
    pub fn new(store: Arc<CredentialStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<CredentialStore> {
        &self.store
    }
}

impl SecretResolver for CredentialStoreAdapter {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<Credential, SecretResolutionError> {
        let record = self
            .store
            .list()
            .into_iter()
            .find(|record| matches_secret_ref(record, secret_ref))
            .ok_or_else(|| SecretResolutionError::Missing {
                provider_id: secret_ref.provider_id.clone(),
                account: AccountDescription(secret_ref.account_id.clone()),
            })?;

        if record
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Err(SecretResolutionError::Expired {
                provider_id: secret_ref.provider_id.clone(),
                account: AccountDescription(secret_ref.account_id.clone()),
            });
        }

        if codegg_config::encryption::get_master_key().is_none() {
            return Err(SecretResolutionError::MasterKeyMissing {
                provider_id: secret_ref.provider_id.clone(),
            });
        }

        let secret = self
            .store
            .get_plaintext(
                &secret_ref.provider_id,
                secret_ref.account_id.as_deref(),
                |_| true,
            )
            .map_err(SecretResolutionError::Store)?
            .ok_or_else(|| SecretResolutionError::Missing {
                provider_id: secret_ref.provider_id.clone(),
                account: AccountDescription(secret_ref.account_id.clone()),
            })?;

        Ok(Credential {
            kind: record.kind,
            secret,
            expires_at: record.expires_at,
        })
    }
}

/// Compatibility name for the store-backed secret resolver.
pub type CredentialStoreSecretResolver = CredentialStoreAdapter;
/// Compatibility name for code that models the adapter as a resolver.
pub use SecretResolver as CredentialResolver;

/// Errors returned when a persisted connection cannot be constructed.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("invalid provider connection descriptor: {0}")]
    InvalidDescriptor(String),
    #[error("connection credential unavailable: {0}")]
    Credential(#[from] SecretResolutionError),
    #[error("credential kind '{kind:?}' is not supported by provider '{provider_id}'")]
    UnsupportedCredentialKind {
        provider_id: String,
        kind: CredentialKind,
    },
}

/// A daemon-facing provider construction seam.
pub trait ProviderFactory: Send + Sync {
    fn build(
        &self,
        descriptor: &ProviderConnectionDescriptor,
    ) -> Result<Box<dyn Provider>, ConnectionError>;
}

/// Default factory backed by a pluggable secret resolver.
///
/// Creating this factory is side-effect free.  Credential lookup, decryption,
/// HTTP client construction, and provider construction happen only in
/// [`ProviderFactory::build`].
pub struct ProviderConnectionFactory {
    resolver: Arc<dyn SecretResolver>,
}

impl ProviderConnectionFactory {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self { resolver }
    }

    pub fn from_store(store: Arc<CredentialStore>) -> Self {
        Self::new(Arc::new(CredentialStoreAdapter::new(store)))
    }
}

impl ProviderFactory for ProviderConnectionFactory {
    fn build(
        &self,
        descriptor: &ProviderConnectionDescriptor,
    ) -> Result<Box<dyn Provider>, ConnectionError> {
        descriptor.validate()?;
        let credential = self.resolver.resolve(&descriptor.secret_ref)?;
        let provider_id = descriptor.provider.implementation_id().to_string();
        let name = descriptor
            .display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| descriptor.provider.default_name());

        match &descriptor.provider {
            ProviderKind::OpenAi => {
                require_api_key(&provider_id, &credential)?;
                let mut config = OpenAiConfig::default_with_key(credential.secret);
                if let Some(base_url) = descriptor.base_url.clone() {
                    config.base_url = base_url;
                }
                config.provider_id = provider_id;
                config.provider_name = name.to_string();
                Ok(Box::new(OpenAiProvider::new(config)))
            }
            ProviderKind::Anthropic => {
                require_api_key(&provider_id, &credential)?;
                let mut provider = AnthropicProvider::new(credential.secret)
                    .with_id(provider_id)
                    .with_name(name.to_string());
                if let Some(base_url) = descriptor.base_url.clone() {
                    provider = provider.with_base_url(base_url);
                }
                Ok(Box::new(provider))
            }
            ProviderKind::Google => {
                require_api_key(&provider_id, &credential)?;
                Ok(Box::new(crate::google::GoogleProvider::new(
                    credential.secret,
                )))
            }
            ProviderKind::AzureOpenAi => {
                require_api_key(&provider_id, &credential)?;
                let endpoint = descriptor.base_url.as_deref().ok_or_else(|| {
                    ConnectionError::InvalidDescriptor(
                        "azure connections require base_url".to_string(),
                    )
                })?;
                Ok(Box::new(crate::azure::AzureProvider::new(
                    credential.secret,
                    endpoint.to_string(),
                )))
            }
            ProviderKind::OpenAiCompatible { .. } => {
                let base_url = descriptor.base_url.as_deref().ok_or_else(|| {
                    ConnectionError::InvalidDescriptor(
                        "openai-compatible connections require base_url".to_string(),
                    )
                })?;
                Ok(Box::new(OpenAiCompatibleProvider::simple_with_credential(
                    &provider_id,
                    name,
                    credential,
                    base_url,
                )))
            }
        }
    }
}

fn require_api_key(provider_id: &str, credential: &Credential) -> Result<(), ConnectionError> {
    if credential.kind != CredentialKind::ApiKey {
        return Err(ConnectionError::UnsupportedCredentialKind {
            provider_id: provider_id.to_string(),
            kind: credential.kind,
        });
    }
    Ok(())
}

fn matches_secret_ref(record: &StoredCredentialRecord, secret_ref: &SecretRef) -> bool {
    record.provider_id == secret_ref.provider_id && record.account_id == secret_ref.account_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration};
    use std::sync::Mutex;
    use tempfile::tempdir;

    struct EnvGuard {
        _guard: std::sync::MutexGuard<'static, ()>,
        old_master: Option<String>,
        old_encryption: Option<String>,
        old_opencode: Option<String>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let guard = crate::auth_types::test_support::lock_env();
            let old_master = std::env::var("CODEGG_MASTER_KEY").ok();
            let old_encryption = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
            let old_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
            std::env::remove_var("CODEGG_MASTER_KEY");
            std::env::remove_var("CODEGG_ENCRYPTION_KEY");
            std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
            Self {
                _guard: guard,
                old_master,
                old_encryption,
                old_opencode,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore_env("CODEGG_MASTER_KEY", self.old_master.take());
            restore_env("CODEGG_ENCRYPTION_KEY", self.old_encryption.take());
            restore_env("OPENCODE_ENCRYPTION_KEY", self.old_opencode.take());
        }
    }

    fn restore_env(name: &str, value: Option<String>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }

    fn stored_adapter(
        expires_at: Option<DateTime<Utc>>,
    ) -> (tempfile::TempDir, CredentialStoreAdapter) {
        let dir = tempdir().expect("tempdir");
        let store =
            Arc::new(CredentialStore::at_path(dir.path().join("credentials.json")).expect("store"));
        std::env::set_var("CODEGG_MASTER_KEY", "connection-test-master");
        store
            .put(
                "openai",
                Some("work"),
                CredentialKind::ApiKey,
                "connection-secret",
                expires_at,
                vec![],
            )
            .expect("put credential");
        std::env::remove_var("CODEGG_MASTER_KEY");
        (dir, CredentialStoreAdapter::new(store))
    }

    #[test]
    fn missing_account_is_explicit_and_does_not_fallback() {
        let _env = EnvGuard::new();
        let (_dir, adapter) = stored_adapter(None);
        let error = adapter
            .resolve(&SecretRef::new("openai", Some("personal".to_string())))
            .unwrap_err();
        assert!(matches!(error, SecretResolutionError::Missing { .. }));
    }

    #[test]
    fn expired_credential_is_reported_before_decryption() {
        let _env = EnvGuard::new();
        let (_dir, adapter) = stored_adapter(Some(Utc::now() - Duration::minutes(1)));
        let error = adapter
            .resolve(&SecretRef::new("openai", Some("work".to_string())))
            .unwrap_err();
        assert!(matches!(error, SecretResolutionError::Expired { .. }));
    }

    #[test]
    fn existing_credential_requires_master_key() {
        let _env = EnvGuard::new();
        let (_dir, adapter) = stored_adapter(None);
        let error = adapter
            .resolve(&SecretRef::new("openai", Some("work".to_string())))
            .unwrap_err();
        assert!(matches!(
            error,
            SecretResolutionError::MasterKeyMissing { .. }
        ));
    }

    #[test]
    fn adapter_round_trip_preserves_kind_and_expiry_without_exposing_secret() {
        let _env = EnvGuard::new();
        let dir = tempdir().expect("tempdir");
        let store =
            Arc::new(CredentialStore::at_path(dir.path().join("credentials.json")).expect("store"));
        let expires_at = Utc::now() + Duration::hours(1);
        std::env::set_var("CODEGG_MASTER_KEY", "connection-test-master");
        store
            .put(
                "compatible",
                None,
                CredentialKind::BearerToken,
                "connection-secret",
                Some(expires_at),
                vec![],
            )
            .expect("put credential");
        let adapter = CredentialStoreAdapter::new(store);
        let credential = adapter
            .resolve(&SecretRef::provider("compatible"))
            .expect("resolve");
        assert_eq!(credential.kind, CredentialKind::BearerToken);
        assert_eq!(credential.expires_at, Some(expires_at));
        assert_eq!(credential.secret, "connection-secret");

        let serialized = serde_json::to_string(&ProviderConnectionDescriptor::new(
            "connection-1",
            ProviderKind::OpenAi,
            SecretRef::provider("openai"),
        ))
        .expect("serialize descriptor");
        assert!(!serialized.contains("connection-secret"));
        assert!(!serialized.contains("encrypted_secret"));
    }

    struct FakeResolver {
        calls: Mutex<usize>,
        credential: Credential,
    }

    impl SecretResolver for FakeResolver {
        fn resolve(&self, _secret_ref: &SecretRef) -> Result<Credential, SecretResolutionError> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.credential.clone())
        }
    }

    #[test]
    fn factory_is_lazy_and_builds_native_and_compatible_providers() {
        let resolver = Arc::new(FakeResolver {
            calls: Mutex::new(0),
            credential: Credential::api_key("factory-secret"),
        });
        let factory = ProviderConnectionFactory::new(resolver.clone());
        assert_eq!(*resolver.calls.lock().unwrap(), 0);

        let openai = ProviderConnectionDescriptor::openai(
            "openai-connection",
            SecretRef::provider("openai"),
        );
        assert_eq!(factory.build(&openai).expect("openai").id(), "openai");

        let anthropic = ProviderConnectionDescriptor::anthropic(
            "anthropic-connection",
            SecretRef::provider("anthropic"),
        );
        assert_eq!(
            factory.build(&anthropic).expect("anthropic").id(),
            "anthropic"
        );

        let compatible = ProviderConnectionDescriptor::openai_compatible(
            "gateway-connection",
            "gateway",
            SecretRef::provider("gateway"),
            "https://gateway.example/v1",
        );
        assert_eq!(
            factory.build(&compatible).expect("compatible").id(),
            "gateway"
        );

        let google = ProviderConnectionDescriptor::new(
            "google-connection",
            ProviderKind::Google,
            SecretRef::provider("google"),
        );
        assert_eq!(factory.build(&google).expect("google").id(), "google");

        let azure = ProviderConnectionDescriptor::new(
            "azure-connection",
            ProviderKind::AzureOpenAi,
            SecretRef::provider("azure"),
        )
        .with_base_url("https://azure.example");
        assert_eq!(factory.build(&azure).expect("azure").id(), "azure");

        assert_eq!(*resolver.calls.lock().unwrap(), 5);
    }

    #[test]
    fn descriptor_validation_rejects_secret_bearing_endpoint_metadata() {
        let descriptor =
            ProviderConnectionDescriptor::openai("connection-1", SecretRef::provider("openai"))
                .with_base_url("https://api.example/v1?api_key=secret");
        let error = descriptor.validate().unwrap_err();
        assert!(matches!(error, ConnectionError::InvalidDescriptor(_)));
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn openai_and_anthropic_reject_bearer_tokens_like_existing_registration() {
        let resolver = Arc::new(FakeResolver {
            calls: Mutex::new(0),
            credential: Credential::bearer("token", None),
        });
        let factory = ProviderConnectionFactory::new(resolver);
        let descriptor =
            ProviderConnectionDescriptor::openai("connection-1", SecretRef::provider("openai"));
        let error = match factory.build(&descriptor) {
            Ok(_) => panic!("bearer token should be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ConnectionError::UnsupportedCredentialKind { .. }
        ));
    }

    #[test]
    fn descriptor_json_is_stable_and_secret_free() {
        let descriptor = ProviderConnectionDescriptor::openai_compatible(
            "connection-1",
            "gateway",
            SecretRef::new("gateway", Some("work".to_string())),
            "https://gateway.example/v1",
        )
        .with_display_name("Gateway");
        let value: serde_json::Value = serde_json::to_value(descriptor).expect("serialize");
        assert_eq!(value["connection_id"], "connection-1");
        assert_eq!(value["provider"]["type"], "openai_compatible");
        assert_eq!(value["secret_ref"]["account_id"], "work");
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert!(!value.to_string().contains("connection-secret"));
        assert!(!value.to_string().contains("encrypted_secret"));
    }
}
