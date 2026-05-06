# Provider Module Override

This file contains provider-specific guidance and overrides root AGENTS.md.

## Token Estimation

Token estimation uses `TokenizerType` enum with model-specific multipliers:

- Claude models: 1.4x multiplier
- Gemini models: 1.2x multiplier
- OpenAI models: 1.0x (cl100k_base)

Use `TokenizerType::for_model(model_name)` to detect the type.