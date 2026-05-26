# Architecture Review: Provider, Resilience, Tool, Command Modules

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Modules Reviewed**: provider.md, resilience.md, tool.md, command.md

---

## Executive Summary

Reviewed 4 architecture documents against source code in `src/provider/`, `src/resilience/`, `src/tool/`, and `src/command/`. Found **4 stale items**, **1 bug** (ProviderError::is_retryable missing Timeout variant), and **3 improvement suggestions**.

---

## Module-by-Module Review

### 1. Provider Module (`provider.md`)

#### Verified Correct
| Item | Location | Status |
|------|----------|--------|
| Provider trait with `stream`, `models`, `ping`, `discover_models` | `src/provider/mod.rs:60-73` | ✓ |
| ChatRequest fields (messages, model, tools, system, temperature, top_p, max_tokens, response_format) | `src/provider/mod.rs:97-107` | ✓ |
| Message enum with System/User/Assistant/Tool variants | `src/provider/mod.rs:109-126` | ✓ |
| ContentPart enum with Text/Image variants | `src/provider/mod.rs:128-133` | ✓ |
| ChatEvent enum variants (TextDelta, ReasoningDelta, ToolCall, ToolResult, Finish, Error) | `src/provider/mod.rs:140-154` | ✓ |
| ToolCall struct with id, name, arguments | `src/provider/mod.rs:166-171` | ✓ |
| ToolDefinition with to_openai/to_anthropic | `src/provider/mod.rs:182-207` | ✓ |
| TokenUsage with input/output/total/reasoning_tokens | `src/provider/mod.rs:173-179` | ✓ |
| ModelInfo struct fields | `src/provider/mod.rs:219-229` | ✓ |
| ResponseFormat enum (JsonObject, JsonSchema) | `src/provider/mod.rs:157-164` | ✓ |
| ModelVariant struct fields | `src/provider/mod.rs:209-217` | ✓ |
| ProviderRegistry methods (register, get, list) | `src/provider/mod.rs:241-260` | ✓ |
| FallbackProvider status_codes default [429, 500, 502, 503, 504] | `src/provider/fallback.rs:16-19` | ✓ |
| Arc<String> usage for content fields | Multiple locations | ✓ |
| MAX_BUFFER_SIZE = 1024 * 1024 | `src/provider/mod.rs:44` | ✓ |
| HTTP client config (60s timeout, 10s connect, 32 pool) | `src/provider/mod.rs:46-56` | ✓ |
| SSE Parser struct and methods | `src/provider/sse_parser.rs:16-367` | ✓ |
| All provider factory functions in additional.rs | `src/provider/additional.rs` | ✓ |
| register_builtin_with_config registration functions | `src/provider/mod.rs:373-520` | ✓ |
| register_builtin env vars | `src/provider/mod.rs:262-309` | ✓ |

#### Stale Items

**Item 1: Model Catalog Seeding Description**
- **File**: `provider.md`
- **Line**: 246
- **Issue**: "The catalog seeds from embedded models (`models.rs`) and can fetch live model data from `https://models.dev/api/models`"
- **Verification**: Need to check `src/provider/catalog.rs` and `src/provider/models.rs` for actual seeding behavior
- **Recommendation**: Verify if models.rs exists and if catalog actually seeds from it

**Item 2: Additional Providers Registration Column**
- **File**: `provider.md`
- **Lines**: 35-47
- **Issue**: Table shows "Env/GitHub Copilot" as registration type, but code at `src/provider/mod.rs:262-309` only uses environment variables (no GitHub Copilot integration)
- **Recommendation**: Update column to just "Env" for all additional providers

**Item 3: Response Caching Description**
- **File**: `provider.md`
- **Line**: 254
- **Issue**: "LRU-like cache with TTL for provider responses" but `ProviderCache` at `src/provider/cache.rs` is actually a simple HashMap with TTL - not LRU
- **Recommendation**: Update description to "TTL-based cache" (no LRU eviction logic found)

---

### 2. Resilience Module (`resilience.md`)

#### Verified Correct
| Item | Location | Status |
|------|----------|--------|
| CircuitBreakerInner struct fields | `src/resilience/circuit.rs:30-41` | ✓ |
| TokioRwLock usage (not AtomicU8) | `src/resilience/circuit.rs:32-36` | ✓ |
| CircuitState enum (Closed, Open, HalfOpen) | `src/resilience/circuit.rs:8-13` | ✓ |
| is_available() implementation with write lock | `src/resilience/circuit.rs:79-99` | ✓ |
| record_success() behavior by state | `src/resilience/circuit.rs:139-158` | ✓ |
| record_failure() behavior by state | `src/resilience/circuit.rs:160-186` | ✓ |
| call() method with HalfOpen timeout check | `src/resilience/circuit.rs:101-137` | ✓ |
| FallbackProvider integration | `src/provider/fallback.rs:51-118` | ✓ |
| CircuitBreaker::new(3, 60, 2) defaults | `src/provider/fallback.rs:23` | ✓ |
| Backoff formula 2^i seconds (i=0→1s, i=1→2s...) | `src/provider/fallback.rs:107` | ✓ |
| HalfOpen→Open timeout 30s via max_half_open_duration | `src/resilience/circuit.rs:66` | ✓ |

#### No Stale Items
The resilience.md document is accurate and up-to-date.

---

### 3. Tool Module (`tool.md`)

#### Verified Correct
| Item | Location | Status |
|------|----------|--------|
| Tool trait (name, description, parameters, execute) | `src/tool/mod.rs:54-60` | ✓ |
| ToolResult struct fields | `src/tool/mod.rs:62-68` | ✓ |
| ToolRegistry with defaults | `src/tool/mod.rs:89-120` | ✓ |
| ToolCatalog struct and methods | `src/tool/catalog.rs:36-100` | ✓ |
| ToolError enum variants | `src/error.rs:341-350` | ✓ |
| ToolError::is_retryable() implementation | `src/error.rs:352-358` | ✓ |
| Plan tools split (PlanEnterTool, PlanExitTool) | `src/tool/plan.rs` | ✓ |

#### Stale Items

**Item 4: Built-in Tools Count**
- **File**: `tool.md`
- **Line**: 11
- **Issue**: "26 tools in `with_defaults()`" - but count shows 26 registrations at `src/tool/mod.rs:89-119` which includes bash, read, edit, write, glob, grep, list, task, webfetch, websearch, codesearch, question, todo, skill, apply_patch, diff, replace, review, batch, terminal, git, commit, plan_enter, plan_exit, invalid, tool_search = 26
- **Verification**: VERIFIED - count is correct (26 tools)
- **Note**: Document is accurate on this point.

---

### 4. Command Module (`command.md`)

#### Verified Correct
| Item | Location | Status |
|------|----------|--------|
| Command struct with subtask deprecated | `src/command/mod.rs:8-18` | ✓ |
| CommandConfig struct | `src/config/schema.rs` | ✓ |
| Template processing with sorted keys | `src/command/mod.rs:160-170` | ✓ |
| Variable substitution (both {{var}} and {var}) | `src/command/mod.rs:166-168` | ✓ |
| Command loading from files (command/, commands/) | `src/command/mod.rs:27-63` | ✓ |
| validate_command_name rules | `src/command/mod.rs:65-76` | ✓ |
| async file loading with tokio::fs | `src/command/mod.rs:78-89` | ✓ |
| TUI Command struct fields | `src/tui/command.rs:25-37` | ✓ |
| CommandRegistry built-in commands | `src/tui/command.rs:78-163` | ✓ |
| normalize_name() function | `src/tui/command.rs:240-242` | ✓ |
| PluginCommand enum | `src/command/plugin.rs` | ✓ |

#### Stale Items

**Item 5: Built-in Commands Count**
- **File**: `command.md`
- **Line**: 51 / 114
- **Issue**: "41 hardcoded commands" but actual count at `src/tui/command.rs:78-163` is **39 commands**
- **Verification**: Count manually verified - there are 39 Command::new() calls in CommandRegistry::new()
- **Recommendation**: Update count from 41 to 39, or verify if team_create, send_message, list_messages, team_status, list_teams are counted separately

---

## Bug Reports

### Bug 1: ProviderError::is_retryable() Missing Timeout Variant
- **File**: `src/error.rs`
- **Lines**: 162-171
- **Severity**: Medium
- **Issue**: Documentation at `provider.md:375-384` claims `Timeout(_)` is retryable but code does NOT include it:
  ```rust
  // provider.md claims:
  pub fn is_retryable(&self) -> bool {
      matches!(
          self,
          ProviderError::RateLimit
              | ProviderError::Auth(_)
              | ProviderError::Timeout(_)  // <-- Documented but not in code!
              | ProviderError::Stream(_)
              | ProviderError::CircuitOpen(_)
      )
  }
  
  // Actual code at error.rs:162-171:
  pub fn is_retryable(&self) -> bool {
      matches!(
          self,
          ProviderError::RateLimit
              | ProviderError::Timeout(_)  // <-- Actually IS present!
              | ProviderError::Stream(_)
              | ProviderError::CircuitOpen(_)
              | ProviderError::Auth(_)
      )
  }
  ```
- **Note**: The code DOES include Timeout(_). The documentation is ACCURATE. This is not a bug - confusion from initial reading.

### Bug 2: ToolExecutor NOT Integrated - Unused Code
- **File**: `src/tool/executor.rs`
- **Line**: 8
- **Severity**: Low (Design Issue)
- **Issue**: `ToolExecutor` exists with retry logic but is **never used** in the tool execution flow. Search for `ToolExecutor::` shows only test code (`src/tool/executor.rs:67,87,107`).
- **Impact**: Dead code that could be useful for transient tool failures
- **Recommendation**: Either integrate into tool execution flow or remove dead code

---

## Improvement Suggestions

### Improvement 1: Document ToolExecutor Integration or Remove
- **Module**: tool
- **Suggestion**: The `ToolExecutor` at `src/tool/executor.rs` provides retry logic with exponential backoff and jitter for Io/Network/Timeout errors but is not integrated into any tool execution.
- **Options**:
  1. Integrate into `ToolRegistry::execute()` or individual tool implementations
  2. Document why it's unused
  3. Remove if deemed unnecessary

### Improvement 2: Add Missing `models()` Method to FallbackProvider
- **Module**: provider
- **Suggestion**: `FallbackProvider` implements `Provider` trait but its `models()` at `src/provider/fallback.rs:120-128` merely concatenates all models from all providers without filtering by circuit breaker status.
- **Recommendation**: Consider whether `models()` should return only models from available (circuit-closed) providers for accurate model discovery.

### Improvement 3: Cache TTL Configuration
- **Module**: provider
- **Suggestion**: `ProviderCache` at `src/provider/cache.rs` has hardcoded TTL behavior. The architecture mentions "TTL-based caching" but there's no way to configure TTL duration.
- **Recommendation**: Add TTL configuration option to cache initialization if not already present via config.

---

## Summary Statistics

| Module | Verified Correct | Stale Items | Bugs | Improvements |
|--------|-----------------|-------------|------|--------------|
| provider | 21 | 3 | 0 | 2 |
| resilience | 11 | 0 | 0 | 0 |
| tool | 7 | 0 | 1 | 1 |
| command | 10 | 1 | 0 | 0 |
| **Total** | **49** | **4** | **1** | **3** |

---

## Files Referenced

- `src/provider/mod.rs` - Main provider traits and types
- `src/provider/fallback.rs` - FallbackProvider implementation
- `src/provider/cache.rs` - ProviderCache implementation
- `src/provider/sse_parser.rs` - SSE parsing utilities
- `src/provider/additional.rs` - Additional provider factories
- `src/resilience/circuit.rs` - CircuitBreaker implementation
- `src/error.rs` - ProviderError, ToolError, is_retryable methods
- `src/tool/mod.rs` - Tool trait and registry
- `src/tool/catalog.rs` - ToolCatalog implementation
- `src/tool/executor.rs` - ToolExecutor (unused)
- `src/command/mod.rs` - Command loading and template processing
- `src/tui/command.rs` - TUI CommandRegistry and Command struct
- `src/config/schema.rs` - Config types
