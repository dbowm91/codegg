use crate::auth::Credential;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::openai_compatible::{
    OpenAiCompatibleConfig, OpenAiCompatibleProvider, ToolChoice,
};
use crate::provider::{ModelInfo, Provider};

/// xAI models exposed by the OpenAI-compatible endpoint.
fn xai_models() -> Vec<ModelInfo> {
    vec![ModelInfo {
        id: "grok-build-0.1".to_string(),
        name: "Grok Build 0.1".to_string(),
        provider: "xai".to_string(),
        context_window: 256_000,
        max_output_tokens: None,
        supports_tools: true,
        supports_vision: false,
        variants: vec![],
    }]
}

pub fn create_xai(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::new(
        "xai",
        "xAI",
        OpenAiCompatibleConfig {
            credential,
            base_url: "https://api.x.ai/v1".to_string(),
            auth_header: "Authorization".to_string(),
            extra_headers: Vec::new(),
            models: xai_models(),
            tool_choice: ToolChoice::Auto,
        },
    )
}

pub fn create_mistral(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "mistral",
        "Mistral",
        credential,
        "https://api.mistral.ai/v1",
    )
}

pub fn create_groq(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "groq",
        "Groq",
        credential,
        "https://api.groq.com/openai/v1",
    )
}

pub fn create_deepinfra(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "deepinfra",
        "DeepInfra",
        credential,
        "https://api.deepinfra.com/v1/openai",
    )
}

pub fn create_cerebras(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cerebras",
        "Cerebras",
        credential,
        "https://api.cerebras.ai/v1",
    )
}

pub fn create_cohere(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "cohere",
        "Cohere",
        credential,
        "https://api.cohere.ai/compatibility/v1",
    )
}

pub fn create_together(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "together",
        "Together AI",
        credential,
        "https://api.together.xyz/v1",
    )
}

pub fn create_perplexity(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "perplexity",
        "Perplexity",
        credential,
        "https://api.perplexity.ai",
    )
}

pub fn create_venice(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "venice",
        "Venice",
        credential,
        "https://api.venice.ai/api/v1",
    )
}

pub fn create_generalcompute(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "generalcompute",
        "GeneralCompute",
        credential,
        "https://api.generalcompute.com/v1",
    )
}

pub fn create_minimax(api_key: String) -> impl Provider {
    debug_log!(
        "create_minimax: using Anthropic-compatible endpoint at https://api.minimax.io/anthropic"
    );
    let models = vec![
        ModelInfo {
            id: "minimax/minimax-2.7".to_string(),
            name: "minimax/minimax-2.7".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.7-highspeed".to_string(),
            name: "minimax/minimax-2.7-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.5".to_string(),
            name: "minimax/minimax-2.5".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.5-highspeed".to_string(),
            name: "minimax/minimax-2.5-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.1".to_string(),
            name: "minimax/minimax-2.1".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax/minimax-2.1-highspeed".to_string(),
            name: "minimax/minimax-2.1-highspeed".to_string(),
            provider: "minimax".to_string(),
            context_window: 204800,
            max_output_tokens: Some(32000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
    ];
    AnthropicProvider::new(api_key)
        .with_base_url("https://api.minimax.io/anthropic".to_string())
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

pub fn create_opencode_go(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "opencode_go",
        "OpenCode Go",
        credential,
        "https://opencode.ai/go/v1",
    )
}
