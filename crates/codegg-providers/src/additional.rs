use crate::anthropic::AnthropicProvider;
use crate::auth_types::Credential;
use crate::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider, ToolChoice};
use crate::setup_catalog::{
    CEREBRAS_BASE_URL, COHERE_BASE_URL, DEEPINFRA_BASE_URL, GENERALCOMPUTE_BASE_URL, GROQ_BASE_URL,
    MINIMAX_BASE_URL, MISTRAL_BASE_URL, PERPLEXITY_BASE_URL, TOGETHER_BASE_URL, VENICE_BASE_URL,
    XAI_BASE_URL,
};
use crate::{ModelInfo, Provider};

pub fn create_xai(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::new(
        "xai",
        "xAI",
        OpenAiCompatibleConfig {
            credential,
            base_url: XAI_BASE_URL.to_string(),
            auth_header: "Authorization".to_string(),
            extra_headers: Vec::new(),
            // No compiled-in seed list. xAI is OpenAI-compatible, so the
            // shared bounded discovery populates this from `{base}/models`.
            models: Vec::new(),
            tool_choice: ToolChoice::Auto,
        },
    )
}

pub fn create_mistral(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "mistral",
        "Mistral",
        credential,
        MISTRAL_BASE_URL,
    )
}

pub fn create_groq(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential("groq", "Groq", credential, GROQ_BASE_URL)
}

pub fn create_deepinfra(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "deepinfra",
        "DeepInfra",
        credential,
        DEEPINFRA_BASE_URL,
    )
}

pub fn create_cerebras(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cerebras",
        "Cerebras",
        credential,
        CEREBRAS_BASE_URL,
    )
}

pub fn create_cohere(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cohere",
        "Cohere",
        credential,
        COHERE_BASE_URL,
    )
}

pub fn create_together(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "together",
        "Together AI",
        credential,
        TOGETHER_BASE_URL,
    )
}

pub fn create_perplexity(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "perplexity",
        "Perplexity",
        credential,
        PERPLEXITY_BASE_URL,
    )
}

pub fn create_venice(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "venice",
        "Venice",
        credential,
        VENICE_BASE_URL,
    )
}

pub fn create_generalcompute(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "generalcompute",
        "GeneralCompute",
        credential,
        GENERALCOMPUTE_BASE_URL,
    )
}

pub fn create_minimax(api_key: String) -> impl Provider {
    debug_log!(
        "create_minimax: using Anthropic-compatible endpoint at https://api.minimax.io/anthropic"
    );
    // No compiled-in seed list. MiniMax is Anthropic-compatible; its
    // model set comes from the provider's discovery endpoint, or from
    // an operator-declared additive `models` block in config.
    let models: Vec<ModelInfo> = Vec::new();
    AnthropicProvider::new(api_key)
        .with_base_url(MINIMAX_BASE_URL.to_string())
        .with_id("minimax".to_string())
        .with_name("MiniMax".to_string())
        .with_models(models)
}

pub fn create_sap_ai_core(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("sap_ai_core", "SAP AI Core", &api_key, &base_url)
}

pub fn create_zenmux(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("zenmux", "Zenmux", &api_key, &base_url)
}

pub fn create_kilo(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("kilo", "Kilo", &api_key, &base_url)
}

pub fn create_vercel_ai_gateway(api_key: String, base_url: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "vercel_ai_gateway",
        "Vercel AI Gateway",
        &api_key,
        &base_url,
    )
}

/// OpenCode Go direct provider.
///
/// The requested model resolves to exactly one wire surface through the shared
/// EggPool provider-profile contract, so `/chat/completions`, `/responses`, and
/// `/messages` are all reachable under one connection identity. Endpoint, path,
/// and per-surface auth shape come from the shared profile; the credential,
/// transport, and the stable `x-opencode-session` header stay CodeGG-owned.
pub fn create_opencode_go(credential: Credential) -> impl Provider {
    crate::opencode_go::OpenCodeGoProvider::new(credential)
}
