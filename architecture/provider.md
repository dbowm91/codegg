# Provider Module

The `provider` module provides a unified interface for interacting with various LLM backends.

## Overview

**Location**: `src/provider/`

**Key Responsibilities**:
- Unified interface for LLM backends (Anthropic, OpenAI, Google, etc.)
- Chat request/response handling
- Model catalog and discovery
- Response caching

## Provider Implementations

### Core Providers

| Provider | File | Models |
|----------|------|--------|
| **Anthropic** | `anthropic.rs` | Claude 3.5 Sonnet, Opus, Haiku |
| **OpenAI** | `openai.rs` | GPT-4o, GPT-4 Turbo, GPT-3.5 Turbo |
| **Google** | `google.rs` | Gemini Pro, Flash |
| **Azure** | `azure.rs` | Azure OpenAI models |
| **Vertex** | `vertex.rs` | Google Vertex AI |
| **Bedrock** | `bedrock.rs` | AWS Bedrock (Claude, Llama, Mistral) |
| **OpenRouter** | `openrouter.rs` | Aggregated models |

### Additional Providers

Located in `provider/additional/`:

| Provider | File |
|----------|------|
| Mistral | `mistral.rs` |
| Groq | `groq.rs` |
| DeepInfra | `deepinfra.rs` |
| Cerebras | `cerebras.rs` |
| Cohere | `cohere.rs` |
| TogetherAI | `together.rs` |
| Perplexity | `perplexity.rs` |
| xAI | `xai.rs` |
| Venice | `venice.rs` |
| MiniMax | `minimax.rs` |

## Core Traits and Types

### Provider Trait

```rust
pub trait Provider: Send + Sync {
    fn stream(&self, request: ChatRequest) -> impl Stream<Item = Result<ChatEvent, ProviderError>>;
    fn models(&self) -> Vec<Model>;
    fn ping(&self) -> Result<(), ProviderError>;
}
```

### ChatRequest

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub system: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}
```

### Message Enum

```rust
pub enum Message {
    System { content: String },
    User { content: String },
    Assistant { content: String, tool_calls: Option<Vec<ToolCall>> },
    Tool { content: String, tool_call_id: String },
}
```

### ChatEvent

```rust
pub enum ChatEvent {
    Text { content: String },
    ToolCall { id: String, name: String, input: Value },
    ToolResult { tool_call_id: String, content: String },
    Error { message: String },
    Eom,  // End of message
}
```

### ToolDefinition

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
```

## Key Components

### catalog.rs - Model Catalog

Maintains registry of available models:

```rust
pub struct ModelCatalog {
    models: Vec<Model>,
}
```

### discovery.rs - Provider Discovery

Auto-discovers providers from environment:

- `ANTHROPIC_API_KEY` → Anthropic
- `OPENAI_API_KEY` → OpenAI
- `GOOGLE_API_KEY` → Google

### cache.rs - Response Caching

```rust
pub struct ResponseCache {
    store: Arc<Mutex<LruCache<String, CachedResponse>>>,
}
```

## ProviderRegistry

Central registry for managing provider instances:

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn get(&self, id: &str) -> Option<Box<dyn Provider>>;
    pub fn register(&mut self, id: String, provider: Box<dyn Provider>);
    pub fn names(&self) -> Vec<String>;
}
```

## Token Estimation

```rust
pub fn count_tokens(model: &str, messages: &[Message]) -> usize {
    // Estimates token count for a messages
    // Used for context window management
}
```

**Implementation Note**: Token estimation varies by provider. Claude uses different counting than GPT.

## Interactions

```
AgentLoop
├── ProviderRegistry::get(provider_id)
│   └── Provider::stream(request)
│       └── HTTP request to LLM API
└── Provider events → ChatEvent stream
```

## Configuration

Related config fields:

```toml
[provider]
default = "anthropic"

[providers.anthropic]
api_key = "sk-..."

[providers.openai]
api_key = "sk-..."
```

## Known Implementation Notes

1. **Provider has send-then-discard bug**: `wait_for_response()` in `plan_registry.rs` sends Cancelled then discards the response channel before awaiting

## See Also

- [agent.md](agent.md) - Uses providers for LLM calls
- [provider/AGENTS.override.md](../.codegg/docs/provider/AGENTS.override.md) - Detailed provider patterns
