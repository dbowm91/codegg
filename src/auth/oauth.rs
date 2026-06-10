//! OAuth scaffolding.
//!
//! The first pass does not implement any actual OAuth flow. This module
//! exists so that [`crate::auth::AuthConfig::OAuthDevice`] has a typed
//! home and so that future official flows (e.g. device-code for a provider
//! that documents a public contract) can land here without churn.
//!
//! All entry points return [`AuthError::Unsupported`].

use crate::auth::AuthConfig;
use crate::auth::AuthError;
use crate::auth::Credential;

#[derive(Debug, Clone)]
pub struct OAuthDeviceSpec {
    pub client_id: String,
    pub scopes: Vec<String>,
    pub auth_url: String,
    pub token_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct OAuthDeviceProvider;

impl OAuthDeviceProvider {
    pub fn new() -> Self {
        Self
    }

    /// Validate that the spec is well-formed. Returns the spec name for
    /// diagnostic use.
    pub fn describe(&self, spec: &OAuthDeviceSpec) -> Result<String, AuthError> {
        if spec.client_id.trim().is_empty() {
            return Err(AuthError::Invalid(
                "OAuth device spec is missing client_id".to_string(),
            ));
        }
        if spec.auth_url.trim().is_empty() || spec.token_url.trim().is_empty() {
            return Err(AuthError::Invalid(
                "OAuth device spec is missing auth_url or token_url".to_string(),
            ));
        }
        Ok(format!(
            "oauth-device(client_id={}, scopes={}, auth_url={}, token_url={})",
            spec.client_id,
            spec.scopes.join(","),
            spec.auth_url,
            spec.token_url
        ))
    }

    /// Convert a typed config into a [`OAuthDeviceSpec`].
    pub fn spec_from_config(cfg: &AuthConfig) -> Result<OAuthDeviceSpec, AuthError> {
        match cfg {
            AuthConfig::OAuthDevice {
                client_id,
                scopes,
                auth_url,
                token_url,
            } => Ok(OAuthDeviceSpec {
                client_id: client_id.clone(),
                scopes: scopes.clone(),
                auth_url: auth_url.clone(),
                token_url: token_url.clone(),
            }),
            other => Err(AuthError::Unsupported(format!(
                "{other:?} is not an OAuth device config"
            ))),
        }
    }

    /// Begin a device-code flow. Reserved for future use; the first pass
    /// returns [`AuthError::Unsupported`].
    pub async fn begin_device_flow(
        &self,
        _spec: &OAuthDeviceSpec,
    ) -> Result<DeviceCode, AuthError> {
        Err(AuthError::Unsupported(
            "OAuth device flow is not implemented in this build".to_string(),
        ))
    }

    /// Poll for a token. Reserved for future use; the first pass returns
    /// [`AuthError::Unsupported`].
    pub async fn poll_for_token(
        &self,
        _spec: &OAuthDeviceSpec,
        _device: &DeviceCode,
    ) -> Result<Credential, AuthError> {
        Err(AuthError::Unsupported(
            "OAuth device flow is not implemented in this build".to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_rejects_missing_client_id() {
        let p = OAuthDeviceProvider::new();
        let spec = OAuthDeviceSpec {
            client_id: "".to_string(),
            scopes: vec![],
            auth_url: "https://example/auth".to_string(),
            token_url: "https://example/token".to_string(),
        };
        assert!(matches!(p.describe(&spec), Err(AuthError::Invalid(_))));
    }

    #[test]
    fn begin_device_flow_returns_unsupported() {
        let p = OAuthDeviceProvider::new();
        let spec = OAuthDeviceSpec {
            client_id: "abc".to_string(),
            scopes: vec!["read".to_string()],
            auth_url: "https://example/auth".to_string(),
            token_url: "https://example/token".to_string(),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let err = rt.block_on(p.begin_device_flow(&spec)).unwrap_err();
        assert!(matches!(err, AuthError::Unsupported(_)));
    }
}
