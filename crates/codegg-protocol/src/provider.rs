//! Secret-safe protocol contracts for daemon-owned provider connections.
//!
//! `SecretInput` is deliberately serializable because the local authenticated
//! IPC request must carry the value to the daemon. Its `Debug` and `Display`
//! implementations are redacted so request tracing, test failures, and
//! diagnostics cannot accidentally print the key.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Bounded API-key envelope for the local create request.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretInput(String);

impl SecretInput {
    pub const MAX_LEN: usize = 16 * 1024;

    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() || value.len() > Self::MAX_LEN || value.chars().any(char::is_control) {
            return Err("secret input must be non-empty, bounded, and free of control characters");
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInput(REDACTED)")
    }
}

impl fmt::Display for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret-input>")
    }
}

/// TLS policy selected by the Eggpool connect form.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EggpoolTlsPolicy {
    Required,
    Optional,
    Disabled,
}

/// Scope metadata supplied by a trusted frontend context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EggpoolConnectionScope {
    Personal { owner_id: String },
    Project { project_id: String },
    Deployment { deployment_id: String },
}

/// Secret-free create request. The secret field is only accepted over the
/// daemon's local authenticated IPC boundary.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateEggpoolConnectionRequest {
    pub host: String,
    pub port: Option<u16>,
    pub tls_policy: EggpoolTlsPolicy,
    pub api_key: SecretInput,
    pub display_name: Option<String>,
    pub scope: EggpoolConnectionScope,
    #[serde(default)]
    pub operation_id: Option<String>,
}

impl fmt::Debug for CreateEggpoolConnectionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateEggpoolConnectionRequest")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls_policy", &self.tls_policy)
            .field("api_key", &self.api_key)
            .field("display_name", &self.display_name)
            .field("scope", &self.scope)
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelDto {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_tools: bool,
    pub supports_vision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionHealthDto {
    pub status: String,
    pub reason_code: Option<String>,
    pub checked_at: i64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConnectionSummaryDto {
    pub id: String,
    pub provider_kind: String,
    pub display_name: String,
    pub endpoint: String,
    pub tls_policy: String,
    pub scope: String,
    pub state: String,
    pub revision: u64,
    pub model_count: usize,
    pub catalog_revision: Option<String>,
    pub health: Option<ConnectionHealthDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateEggpoolConnectionResult {
    pub operation_id: String,
    pub connection: ProviderConnectionSummaryDto,
    pub models: Vec<ProviderModelDto>,
    pub catalog_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionProvisioningStatusDto {
    pub operation_id: String,
    pub state: String,
    pub connection_id: Option<String>,
    pub reason_code: Option<String>,
}
