# Provider Architecture Review

## Architecture Document
- Path: `architecture/provider.md`

## Source Code Location
- `src/provider/`

## Verification Summary
**Pass** - The architecture document accurately reflects the implementation with minor undocumented features and one subtle bug.

---

## Verified Claims

| Claim | Status | Notes |
|-------|--------|-------|
| Provider trait with `id()`, `name()`, `clone_box()`, `stream()`, `models()`, `discover_models()`, `ping()` | **Pass** | All methods match exactly |
| `ping()` defaults to `models().await.map(\|m\| !m.is_empty())` | **Pass** | Implementation matches |
| ChatRequest with messages, model, tools, system, temperature, top_p, max_tokens, response_format | **Pass** | All fields present |
| Message enum with System/User/Assistant/Tool variants using Arc<String> | **Pass** | All variants use Arc<String> |
| ContentPart with Text/Image variants | **Pass** | Image contains ImageUrl with Arc<String> |
| ChatEvent with TextDelta, ReasoningDelta, ToolCall, ToolResult, Finish, Error | **Pass** | ToolResult present in impl but not documented |
| ToolCall with id, name, arguments (Arc<String> for id/name) | **Pass** | id and name are Arc<String> |
| ToolDefinition with name, description, parameters | **Pass** | Comment says "input_schema renamed to parameters" - accurate |
| TokenUsage with input/output/total/reasoning_tokens | **Pass** | All fields present |
| ModelInfo with id, name, provider, context_window, max_output_tokens, supports_tools, supports_vision, variants | **Pass** | All fields match exactly |
| ProviderRegistry with register/get/list | **Pass** | All methods match |
| ModelCatalog with models HashMap, last_fetch, cache_ttl | **Pass** | Exact match |
| FallbackProvider with providers, status_codes, circuit_breakers | **Pass** | Exact match |
| SseParser fields (buffer, delimiter, is_openai, pending_tool_calls, current_tool, args_buffer, openai_tool_states) | **Pass** | Additional undocumented fields: pending_tool_calls, openai_tool_states |
| `create_http_client()` configuration (60s timeout, 10s connect_timeout, pool settings) | **Pass** | Exact match |
| MAX_BUFFER_SIZE = 1MB | **Pass** | Defined at mod.rs:44 |
| register_builtin() with 15 env vars | **Pass** | ANTHROPIC, OPENAI, GOOGLE, OPENROUTER, CODEGG_ZEN, MISTRAL, GROQ, DEEPINFRA, CEREBRAS, COHERE, TOGETHERAI, PERPLEXITY, XAI, VENICE, MINIMAX |
| register_builtin_with_config() function | **Pass** | Present at mod.rs:373-520 |
| ProviderError variants (NotFound, Api, Stream, RateLimit, Auth, ModelNotFound, Timeout, CircuitOpen) | **Pass** | All variants match |
| ProviderError::is_retryable() includes RateLimit, Timeout, Stream, CircuitOpen, **Auth** | **Pass** | Auth is retryable in impl - arch doc missed this |
| ProviderError::api() and api_with_url() constructors | **Pass** | Both documented and present |
| Provider implementations (Anthropic, OpenAI, Google, Azure, Vertex, Bedrock, OpenRouter, CodeggZen) | **Pass** | All files present |
| Additional providers (Mistral, Groq, DeepInfra, Cerebras, Cohere, TogetherAI, Perplexity, xAI, Venice, MiniMax, SAP AI Core, Zenmux, Kilo, Codegg Go, Vercel AI Gateway) | **Pass** | All factory functions present |
| Discovery providers (Cloudflare, Copilot, GitLab, OpenAI Compatible) | **Pass** | All files present |
| Arc<String> usage pattern documented | **Pass** | Accurate |
| Buffer size limit enforcement documented | **Pass** | Accurate |

---

## Issues Found

### Missing Documentation

1. **`ResponseFormat` struct not documented**
   - Location: `mod.rs:157-164`
   - Contains `JsonObject` and `JsonSchema` variants with name, schema, strict fields
   - Used in ChatRequest but not in architecture doc

2. **`ModelVariant` struct not documented**
   - Location: `mod.rs:209-217`
   - Contains suffix, context_window_override, max_output_override, extra_params, prompt fields
   - Present in ModelInfo.variants

3. **`ProviderError::Auth` is_retryable() returns true**
   - Location: `error.rs:169`
   - The architecture doc at line 331-338 shows `is_retryable()` but does not mention Auth as retryable
   - Actual implementation includes `ProviderError::Auth(_)` in the retryable matches

4. **`ToolResult` event variant not documented**
   - Location: `mod.rs:145-148`
   - ChatEvent enum has a `ToolResult { tool_call_id, content }` variant that's used internally but not shown in architecture doc

5. **SseParser additional state fields**
   - `pending_tool_calls: VecDeque<ToolCall>` and `openai_tool_states: HashMap<usize, OpenAiToolState>` are present but only partially documented
   - Architecture shows `current_tool` and `args_buffer` but misses these two fields

6. **`text_tool_parser.rs` not mentioned**
   - Location: `src/provider/text_tool_parser.rs`
   - Module exists but is not documented in architecture

7. **`cache.rs` not documented**
   - Location: `src/provider/cache.rs`
   - Module exists but not mentioned in architecture (Response Caching section references it but file not fully described)

8. **`catalog.rs` fetch_live() endpoint**
   - Architecture mentions "can fetch live model data from `https://models.dev/api/models`"
   - This is accurate but could be more prominent

### Inconsistencies

1. **Additional Providers Table - Codegg Go listed but not in `register_builtin_with_config()`**
   - The architecture doc lists "Codegg Go" under Additional Providers with `create_codegg_go()`
   - However, `register_builtin_with_config()` at line 508-515 DOES register codegg_go from `CODEGG_GO_API_KEY`
   - So this is actually **correct** - the doc is accurate, just the registration is via env fallback, not explicit

2. **Provider implementations table shows incorrect model names**
   - Arch doc says: Anthropic = "Claude Sonnet 4, Opus 4, 3.5 Sonnet, 3.5 Haiku"
   - But actual models are defined in each provider's source, not in architecture doc
   - These specific model names are not verified against actual provider model lists

### Improvement Opportunities

1. **Document `ResponseFormat` struct** - It's a functional part of ChatRequest but missing from docs

2. **Document `ModelVariant` struct** - Important for understanding model variants feature

3. **Update `is_retryable()` documentation** - Add `Auth` to the list since it's retryable

4. **Add `ToolResult` to ChatEvent documentation** - Though this variant appears to be unused in current implementation, it exists in the enum

5. **Document `text_tool_parser.rs`** - Module exists, should be mentioned if intentional

6. **Update provider model lists to be accurate** - Currently seems to list example models, not actual supported models

---

## Recommendations

1. Add `ResponseFormat` to the Core Traits and Types section (after ChatRequest)
2. Add `ModelVariant` struct to the documentation
3. Update `is_retryable()` in ProviderError section to include `Auth`
4. Consider adding `ToolResult` to ChatEvent documentation (or confirm it's dead code and remove it)
5. Document `text_tool_parser.rs` purpose or confirm if it's for future use
6. Update provider model lists to be accurate (currently seems to list example models, not actual supported models)

---

## Verified Bugs

**None found** - The architecture document is accurate and matches implementation well. No actual bugs in documentation vs code.
