use crate::auth::Credential;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::openai_compatible::{
    OpenAiCompatibleConfig, OpenAiCompatibleProvider, ToolChoice,
};
use crate::provider::{ModelInfo, Provider};

pub fn create_xai(api_key: String) -> impl Provider {
    let models = vec![ModelInfo {
        id: "grok-build-0.1".to_string(),
        name: "Grok Build 0.1".to_string(),
        provider: "xai".to_string(),
        context_window: 256_000,
        max_output_tokens: None,
        supports_tools: true,
        supports_vision: false,
        variants: vec![],
    }];
    OpenAiCompatibleProvider::new(
        "xai",
        "xAI",
        OpenAiCompatibleConfig {
            credential: Credential::api_key(&api_key),
            base_url: "https://api.x.ai/v1".to_string(),
            auth_header: "Authorization".to_string(),
            extra_headers: Vec::new(),
            models,
            tool_choice: ToolChoice::Auto,
        },
    )
}

pub fn create_mistral(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("mistral", "Mistral", &api_key, "https://api.mistral.ai/v1")
}

pub fn create_groq(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("groq", "Groq", &api_key, "https://api.groq.com/openai/v1")
}

pub fn create_deepinfra(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "deepinfra",
        "DeepInfra",
        &api_key,
        "https://api.deepinfra.com/v1/openai",
    )
}

pub fn create_cerebras(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "cerebras",
        "Cerebras",
        &api_key,
        "https://api.cerebras.ai/v1",
    )
}

pub fn create_cohere(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "cohere",
        "Cohere",
        &api_key,
        "https://api.cohere.ai/compatibility/v1",
    )
}

pub fn create_together(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "together",
        "Together AI",
        &api_key,
        "https://api.together.xyz/v1",
    )
}

pub fn create_perplexity(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "perplexity",
        "Perplexity",
        &api_key,
        "https://api.perplexity.ai",
    )
}

pub fn create_venice(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple("venice", "Venice", &api_key, "https://api.venice.ai/api/v1")
}

pub fn create_generalcompute(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "generalcompute",
        "GeneralCompute",
        &api_key,
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

pub fn create_opencode_go(api_key: String) -> impl Provider {
    OpenAiCompatibleProvider::simple(
        "opencode_go",
        "OpenCode Go",
        &api_key,
        "https://opencode.ai/go/v1",
    )
}
