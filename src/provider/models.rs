use crate::provider::ModelInfo;

pub fn embedded_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "big-pickle".to_string(),
            name: "Big Pickle (Free)".to_string(),
            provider: "opencode_zen".to_string(),
            context_window: 200_000,
            max_output_tokens: Some(64_000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "minimax-m2.5-free".to_string(),
            name: "MiniMax M2.5 Free".to_string(),
            provider: "opencode_zen".to_string(),
            context_window: 200_000,
            max_output_tokens: Some(64_000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "nemotron-3-super-free".to_string(),
            name: "Nemotron 3 Super Free".to_string(),
            provider: "opencode_zen".to_string(),
            context_window: 128_000,
            max_output_tokens: Some(32_000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
        ModelInfo {
            id: "qwen3.6-plus-free".to_string(),
            name: "Qwen3.6 Plus Free".to_string(),
            provider: "opencode_zen".to_string(),
            context_window: 128_000,
            max_output_tokens: Some(32_000),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        },
    ]
}

pub fn find_model(id: &str) -> Option<ModelInfo> {
    embedded_models().into_iter().find(|m| m.id == id)
}
