macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*);
    };
}

pub mod additional;
pub mod anthropic;
pub mod auth_types;
pub mod azure;
pub mod bedrock;
pub mod cache;
pub mod catalog;
pub mod circuit;
pub mod cloudflare;
pub mod connection;
pub mod copilot;
pub mod crypto;
pub mod discovery;
pub mod eggpool;
pub mod error;
pub mod fallback;
pub mod gitlab;
pub mod google;
pub mod models;
pub mod openai;
pub mod openai_compatible;
pub mod opencode_zen;
pub mod openrouter;
pub mod sse_parser;
pub mod text_tool_parser;
pub mod vertex;

pub use auth_types::{
    mask_secret, AuthConfig, AuthError, AuthResolver, Credential, CredentialKind, CredentialStore,
    ExternalCommandProvider, ExternalCredential, ResolvedAuth, ResolvedAuthSource, ResolverContext,
    StoredCredentialRecord,
};
pub use circuit::{CircuitBreaker, CircuitError, CircuitState};
pub use connection::{
    ConnectionDescriptor, ConnectionError, ConnectionKind, CredentialResolver,
    CredentialStoreAdapter, CredentialStoreSecretResolver, ProviderConnection,
    ProviderConnectionDescriptor, ProviderConnectionFactory, ProviderFactory, ProviderKind,
    SecretRef, SecretReference, SecretResolutionError, SecretResolver,
};
pub use eggpool::{
    normalize_eggpool_base_url, EggpoolApiKey, EggpoolCancellationToken, EggpoolModelSummary,
    EggpoolProbe, EggpoolProbeError, EggpoolProbeOptions, EggpoolProbeReasonCode,
    EggpoolProbeSummary, EGGPOOL_DEFAULT_PORT,
};
pub use error::{ProviderError, StorageError};
pub use provider_core::{
    assistant_text_content_value, create_http_client, openai_tool_arguments_value,
    register_builtin, register_builtin_with_config, ChatEvent, ChatRequest, ContentPart,
    EventStream, ImageUrl, Message, ModelInfo, ModelVariant, Provider, ProviderCapabilities,
    ProviderRegistry, ResponseFormat, TokenUsage, ToolCall, ToolDefinition, MAX_BUFFER_SIZE,
};

// The core provider types and registration logic
mod provider_core;
