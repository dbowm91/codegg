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
pub mod responses_api;
pub mod retry;
pub mod setup_catalog;
pub mod sse_parser;
pub mod text_tool_parser;
pub mod vertex;

pub use auth_types::{
    incompatible_credential_message, mask_secret, AuthConfig, AuthError, AuthResolver, Credential,
    CredentialCapability, CredentialKind, CredentialStore, ExternalCommandProvider,
    ExternalCredential, ResolvedAuth, ResolvedAuthSource, ResolverContext, StoredCredentialRecord,
};
pub use circuit::{CircuitBreaker, CircuitError, CircuitState};
pub use connection::{
    capability_for_provider_kind, validate_rotation_kind, ConnectionDescriptor, ConnectionError,
    ConnectionKind, CredentialResolver, CredentialStoreAdapter, CredentialStoreSecretResolver,
    ProviderConnection, ProviderConnectionDescriptor, ProviderConnectionFactory, ProviderFactory,
    ProviderKind, SecretRef, SecretReference, SecretResolutionError, SecretResolver,
};
pub use eggpool::{
    normalize_eggpool_base_url, EggpoolApiKey, EggpoolCancellationToken, EggpoolModelSummary,
    EggpoolProbe, EggpoolProbeError, EggpoolProbeOptions, EggpoolProbeReasonCode,
    EggpoolProbeSummary, EGGPOOL_DEFAULT_PORT,
};
pub use error::{ProviderError, RetryDisposition, StorageError, MAX_RETRY_AFTER_HINT};
pub use provider_core::{
    assistant_text_content_value, builtin_registration_order, create_http_client,
    credential_capability_for, non_streaming_timeout, openai_tool_arguments_value,
    project_tool_call_history, register_builtin, register_builtin_with_config, ChatEvent,
    ChatRequest, ContentPart, EventStream, ImageUrl, Message, ModelInfo, ModelVariant, Provider,
    ProviderCapabilities, ProviderCredentialCapability, ProviderRegistry, ProviderRequestContext,
    ReasoningVisibility, ResponseFormat, TokenUsage, ToolCall, ToolDefinition, MAX_BUFFER_SIZE,
    MAX_REASONING_BYTES, NON_STREAMING_PROVIDER_TOTAL,
};
pub use responses_api::{
    filter_artifacts_for_provider, validate_arguments, validate_call_count, validate_result_size,
    ArtifactRef, CompletedHostedCall, ContinuationState, HostedBackendPolicy, HostedCallIdentity,
    HostedProgramAdapter, HostedProgramEvent, HostedProgramMetadata, HostedUsage, InputValidation,
    ReservedCall, ResolvedBackend, ResponseItem, ResponseObject, ResponsesRequest,
    ResponsesStreamEvent, ResponsesTool, ResponsesTransport, ResponsesTransportConfig,
    ResponsesUsage, DEFAULT_REQUEST_TIMEOUT, DEFAULT_STREAM_IDLE_TIMEOUT, MAX_ARGUMENT_SIZE,
    MAX_INPUT_BODY_SIZE, MAX_NESTED_CALLS, MAX_RESPONSE_ITEMS, MAX_RESULT_SIZE,
    MAX_SSE_BUFFER_SIZE,
};
pub use retry::{
    AckState, ReconciliationOutcome, RetryChainId, RetryContext, RetryContextDto,
    UncertainSideEffect, UnifiedRetryDisposition, MAX_CHAIN_ATTEMPTS, MAX_CHAIN_DURATION,
    MAX_UNCERTAIN_DETAIL_CHARS,
};
pub use setup_catalog::{
    build_durable_provider, fixed_base_url, provider_setup_catalog, setup_definition,
    ProviderDefinition, SetupConstruction, SetupEndpointPolicy, SetupProbeStrategy,
    ANTHROPIC_BASE_URL, AZURE_ID, CEREBRAS_BASE_URL, COHERE_BASE_URL, CUSTOM_COMPATIBLE_ID,
    DEEPINFRA_BASE_URL, EGGPOOL_PRESET_DEFAULT_PORT, EGGPOOL_PRESET_ID, GENERALCOMPUTE_BASE_URL,
    GOOGLE_ENDPOINT, GROQ_BASE_URL, MINIMAX_BASE_URL, MISTRAL_BASE_URL, OPENAI_BASE_URL,
    OPENCODE_GO_BASE_URL, OPENCODE_ZEN_BASE_URL, OPENROUTER_ENDPOINT, PERPLEXITY_BASE_URL,
    TOGETHER_BASE_URL, VENICE_BASE_URL, XAI_BASE_URL,
};

// The core provider types and registration logic
mod provider_core;
