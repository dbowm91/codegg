# Tool Use Enhancement Plan

**Status**: PARTIALLY COMPLETE (verified 2026-06-02)

| Part | Status | Notes |
|------|--------|-------|
| Part 1: Tool Use Optimization (Steps 1-6, Phases 1-3) | **DONE** | `defer_loading`, ToolCatalog, ProviderCapabilities, BM25, MCP integration |
| Part 2: eggsact Integration (Phase 4) | **DEFERRED** | External crate, separate project |
| Part 3: Embeddings Search (Phase 5) | **DEFERRED** | Optional v3 upgrade path |
| Configuration schema | **DONE** | `ToolDeferralConfig` in config |

**Last Updated**: 2026-05-07

---

## Executive Summary

This plan covers two related efforts:

1. **Immediate**: Optimize current opencode-rs tool exposure using Claude Code's Tool Search patterns
2. **Future**: Integrate the `eggsact` crate (Rust rewrite of nl-clicalc) as native built-in tools

The core challenge is balancing **context efficiency** (too many tools pollutes context) against **capability awareness** (agent must know tools exist to use them). Research from Anthropic, OpenAI, and industry consensus shows **deferred/lazy tool loading** is the solution - tools are discovered on-demand rather than front-loaded.

---

## Part 1: Current Tool Use Optimization

### Research Findings

#### Claude Code Tool Search Pattern

Claude Code's Tool Search enables **deferred tool loading** - tool definitions are withheld from context until needed. Key findings:

**Token Savings (Anthropic Data)**:
- Traditional 50+ MCP tools: ~72K tokens before work begins
- With Tool Search enabled: ~8.7K tokens
- **85% reduction**

**How It Works**:
1. At session start, model sees only `tool_search_tool_regex` or `tool_search_tool_bm25` tool
2. All other tools marked `defer_loading: true` in API request
3. When model needs a tool, it calls search with natural language query
4. API returns `tool_reference` blocks which auto-expand to full definitions

**Critical Rule**: The tool search tool itself must NEVER have `defer_loading: true`. At least one non-search tool must be non-deferred.

**Two Search Variants**:
- `tool_search_tool_regex_20251119`: Uses Python regex syntax (`re.search()`)
- `tool_search_tool_bm25_20251119`: Natural language queries

**OpenAI's Approach**:
- Soft limit: "fewer than 20 functions available at the start of a turn"
- `defer_loading: true` support for gpt-5.4+
- Client-side tool search returns `tool_reference` blocks

#### Critical Discovery: opencode-rs Already Has Foundation

Our codebase already has:

```rust
// src/tool/catalog.rs - EXISTS
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,  // Already here!
}

pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}

// src/tool/tool_search.rs - EXISTS
pub struct ToolSearchTool { ... }
```

**The foundation exists.** We need to wire it up properly.

#### Gap Analysis

| Component | Current State | Needed |
|-----------|---------------|--------|
| `ToolDefinition` (provider/mod.rs) | `name`, `description`, `parameters` | Add `defer_loading: Option<bool>` |
| ToolRegistry registration | `register(tool)` | Accept `defer_load` parameter |
| `tool_search` tool | Exists, registered | Must be explicitly non-deferred |
| Provider requests | Single `tools` array | Separate `deferred_tools` for API compatibility |
| MCP tools | Listed via `McpService` | Integrate with catalog deferral |

### Implementation Steps

#### Step 1: Update ToolDefinition (provider/mod.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub defer_loading: Option<bool>,  // None = not deferred
}
```

#### Step 2: Update ToolRegistry Registration

```rust
// src/tool/mod.rs
pub fn register(&mut self, tool: impl Tool + 'static, defer_load: bool) {
    let name = tool.name().to_string();
    self.catalog.register(&tool, defer_load);  // pass defer_load
    self.tools.insert(name, Box::new(tool));
}
```

#### Step 3: Ensure tool_search is Always-Loaded

```rust
// In with_defaults():
registry.register(crate::tool::tool_search::ToolSearchTool::new(), false);  // defer_load = false
```

#### Step 4: Separate Immediate vs Deferred Tools

In `AgentLoop::build_tools()`:

```rust
let (immediate, deferred): (Vec<_>, Vec<_>) = all_tools.iter()
    .partition(|t| t.defer_loading != Some(true));

request.tools = Some(immediate);
request.deferred_tools = Some(deferred);  // For providers that support it
```

#### Step 5: Provider-Level Handling

For Anthropic provider:
```rust
// Send defer_loading in tool definitions
// deferred_tools separate array for explicit deferred loading
```

For OpenAI provider:
```rust
// Use tool_search for deferred if gpt-5.4+
// Otherwise merge all tools (no deferral support)
```

For providers without deferral support:
```rust
// Fall back: include all tools in single array
// Tool search still works client-side for discovery
```

#### Step 6: MCP Tool Integration

When `McpService.list_tools()` builds tool definitions:

```rust
pub fn list_tools(&self) -> Vec<ToolDefinition> {
    self.servers.values().flat_map(|s| {
        s.tools.iter().map(|t| ToolDefinition {
            name: format!("mcp__{}__{}", s.name, t.name),
            description: t.description.clone(),
            parameters: t.input_schema.clone(),
            defer_loading: Some(self.catalog.is_deferred(t.name)),  // respect deferral
        })
    }).collect()
}
```

### Configuration Options

```toml
# config.json
[tool]
# How tools are exposed to the agent
default_defer_load = true      # All tools deferred by default
always_loaded = ["read", "bash", "edit", "write", "tool_search"]  # Core tools always visible
search_enabled = true           # Enable tool_search tool
```

### Testing Plan

Use existing `agent_loop_harness.rs` to test:

1. **Tool discovery**: Agent uses `tool_search` when stuck
2. **Deferred loading**: Verify deferred tools don't appear in initial context
3. **Full tool exposure**: When configured, all tools visible without deferral
4. **Context efficiency**: Measure token reduction with deferral

---

## Part 2: eggsact Integration (Future)

### eggsact Crate Overview

eggsthought: A Rust crate (separate project) that rewrites nl-clicalc's functionality:

**Core Modules**:
- `eval.rs` - AST-based math evaluation
- `normalize.rs` - Natural language parsing
- `units.rs` - Unit conversion
- `text/` - Text inspection primitives (confusables, measure, validate)
- `mcp/` - Optional MCP server mode

**Tools to Implement** (from nl-clicalc):

| Tool | Purpose | Context for Coding Agents |
|------|---------|---------------------------|
| `math_eval` | Natural language math evaluation | Calculate dimensions, sizes, estimates |
| `text_measure` | UTF-8 bytes, codepoints, words, lines | File size analysis, text processing |
| `text_equal` | String comparison with normalization | Comparing snippets, checking sameness |
| `text_diff_explain` | Human-readable diff explanation | Code review assistance |
| `text_inspect` | Hidden chars, confusables, mixed scripts | **Critical security tool** - detect homoglyph attacks |
| `text_count` | Character frequency counting | Text analysis |
| `validate_brackets` | Match bracket pairs | Syntax checking |
| `validate_json` | JSON syntax validation | Debugging, API work |
| `validate_regex` | Regex pattern testing | Validation |
| `list_compare` | Element-by-element list comparison | Diffing |

**Why text_inspect is Critical**:

The agent stuck for 30 minutes trying to compare two words thinking it was a spelling error. `text_equal` or `text_inspect` would have solved this immediately.

### Integration Architecture

```
eggsact (external crate)
├── src/
│   ├── lib.rs           # Core evaluation (no I/O)
│   ├── eval.rs          # Math AST evaluation
│   ├── normalize.rs     # NL parsing
│   ├── units.rs         # Unit conversion
│   ├── text/            # Text primitives
│   │   ├── confusables.rs
│   │   ├── measure.rs
│   │   └── validate.rs
│   └── mcp/             # Optional MCP server
```

In opencode-rs:

```rust
// src/tool/eggsact.rs
pub struct MathEvalTool { ... }
pub struct TextInspectTool { ... }
pub struct ValidateJsonTool { ... }
// ...

impl Tool for MathEvalTool {
    fn name(&self) -> &str { "math_eval" }
    fn description(&self) -> &str {
        "Evaluate mathematical expressions with support for natural language \
         (e.g., 'five plus three times two'), units (e.g., '30m + 100ft'), \
         complex numbers, and built-in constants (pi, e, avogadro). \
         Use when you need to calculate dimensions, estimates, or any numeric computation."
    }
    fn parameters(&self) -> serde_json::Value { ... }
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let expr = input["expression"].as_str().ok_or_else(|| ...)?;
        let result = eggsact::evaluate(expr)?;
        Ok(result.to_string())
    }
}
```

Registration in `with_defaults()`:

```rust
// Feature-gated: only if eggsact feature enabled
#[cfg(feature = "eggsact")]
{
    registry.register(crate::tool::eggsact::MathEvalTool::default(), true);
    registry.register(crate::tool::eggsact::TextInspectTool::default(), true);
    registry.register(crate::tool::eggsact::ValidateJsonTool::default(), true);
    // ... all eggsact tools
}
```

### Tool Naming Convention

Option A: Match MCP names (math_eval, text_inspect, validate_json)
Option B: Egg-prefixed (egg_math, egg_inspect, egg_validate)
Option C: Unified naming (calc_eval, text_check, json_validate)

**Recommendation**: Option A for maximum compatibility with MCP server version. If user runs eggsact as MCP server elsewhere, tool names should match.

---

## Part 3: Embeddings-Based Search (Scoped)

### What Is Embeddings-Based Search?

Traditional keyword/regex search finds tools by matching exact words. **Semantic search** finds tools by meaning - it understands that "weather" and "forecast" are related even without exact matches.

**How It Works** (from Anthropic cookbook):

1. Convert each tool definition into a vector embedding (e.g., 384 dimensions for all-MiniLM-L6-v2)
2. When agent searches, embed their query
3. Find tools with highest cosine similarity to query
4. Return `tool_reference` blocks for matching tools

**Token Savings**: Embeddings-based search can achieve similar 85%+ reduction as built-in Tool Search, but works client-side for any provider.

### When Is This Needed?

| Approach | Best For | Limitation |
|----------|----------|------------|
| Built-in Tool Search (Anthropic) | Claude directly | Requires Anthropic API |
| `defer_loading` (OpenAI gpt-5.4+) | OpenAI models | Only newer OpenAI models |
| Regex/BM25 search | Simple keyword matching | Misses semantic matches |
| Embeddings-based | Large tool libraries, any provider | Adds latency, complexity |

### Scope for opencode-rs

**Recommended Approach**: BM25/Regex for v1, embeddings upgrade path for v2

**Rationale**: The current `ToolCatalog::search()` uses simple text matching. This works well for 40-50 tools. If/when we grow to hundreds of tools, embeddings become valuable.

**Implementation Path**:

```rust
// src/tool/catalog.rs

pub enum SearchMode {
    /// Simple keyword/regex search (v1)
    Keyword,
    /// BM25 ranking (v2 upgrade path)
    BM25,
    /// Semantic embeddings (v3, requires model)
    Embeddings,
}

pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
    search_mode: SearchMode,
    // For embeddings mode:
    embeddings: Option<EmbeddingsIndex>,  // keyed by tool name
}

impl ToolCatalog {
    pub fn search(&self, query: &str) -> Vec<&ToolMetadata> {
        match self.search_mode {
            SearchMode::Keyword => self.keyword_search(query),
            SearchMode::BM25 => self.bm25_search(query),
            SearchMode::Embeddings => self.embedding_search(query),
        }
    }
}
```

### For Chinese/Non-Standard Providers

Many users target cheaper Chinese models (DeepSeek, Qwen, etc.) which:
- Support function calling
- Do NOT have native `defer_loading` support
- May not understand `tool_reference` blocks

**Fallback Strategy**:

```rust
pub struct ProviderCapabilities {
    supports_defer_loading: bool,
    supports_tool_references: bool,
    max_tools_per_request: Option<usize>,
}

impl ProviderCapabilities {
    pub fn for_provider(provider: &str) -> Self {
        match provider {
            "anthropic" => Self {
                supports_defer_loading: true,
                supports_tool_references: true,
                max_tools_per_request: None,
            },
            "openai" => Self {
                supports_defer_loading: true,  // gpt-5.4+
                supports_tool_references: true,
                max_tools_per_request: Some(128),
            },
            // Chinese models
            "deepseek" | "qwen" | "yi" => Self {
                supports_defer_loading: false,
                supports_tool_references: false,
                max_tools_per_request: Some(30),
            },
            _ => Self {
                supports_defer_loading: false,
                supports_tool_references: false,
                max_tools_per_request: None,
            },
        }
    }
}
```

**For unsupported providers**:
1. Always load all tools (no deferral)
2. Tool search still works client-side but returns full tool definitions instead of `tool_reference`
3. May cause context bloat but maintains functionality

---

## Part 4: Testing Strategy

### Agent Loop Harness Tests

The existing `agent_loop_harness.rs` (~3200 lines) provides comprehensive testing infrastructure.

**New Test Cases**:

```rust
#[tokio::test]
async fn test_tool_search_deferred_loading() {
    // Arrange: Registry with some tools deferred
    let registry = ToolRegistry::with_defaults();
    registry.register(MathEvalTool::default(), true);  // deferred
    registry.register(TextInspectTool::default(), true);  // deferred
    registry.register(ReadTool::default(), false);  // immediate

    // Act: Build tool definitions
    let defs = registry.definitions();

    // Assert: Only read and tool_search are non-deferred
    let immediate: Vec<_> = defs.iter().filter(|t| t.defer_loading != Some(true)).collect();
    assert!(immediate.iter().any(|t| t.name == "read"));
    assert!(immediate.iter().any(|t| t.name == "tool_search"));
    assert!(immediate.iter().all(|t| t.name != "math_eval"));
}

#[tokio::test]
async fn test_tool_search_discovers_deferred() {
    // Agent gets stuck comparing two words
    // Should discover text_equal via tool_search
}
```

### Benchmarks

- **Context tokens**: Measure tokens before work begins with/without deferral
- **Tool selection accuracy**: Does agent pick correct tool?
- **Discovery latency**: How long from search call to tool use?

---

## Part 5: Configuration Schema

```json
{
  "tools": {
    "defer_loading": true,
    "always_loaded": ["read", "bash", "edit", "write", "glob", "grep", "tool_search"],
    "search_mode": "keyword",
    "max_initial_tools": 20,
    "provider_defaults": {
      "anthropic": { "defer_loading": true },
      "openai": { "defer_loading": true },
      "deepseek": { "defer_loading": false },
      "qwen": { "defer_loading": false }
    }
  }
}
```

---

## Implementation Order

### Phase 1: Core Infrastructure (1-2 days)

1. Add `defer_loading` to `ToolDefinition`
2. Update `ToolRegistry::register()` to accept defer flag
3. Ensure `tool_search` is always loaded
4. Add provider capability detection

### Phase 2: Provider Integration (2-3 days)

1. Wire deferred vs immediate tool separation in `AgentLoop::build_tools()`
2. Add Anthropic defer_loading support
3. Add OpenAI fallback for non-supporting models
4. Test with agent_loop_harness

### Phase 3: MCP Integration (1-2 days)

1. Integrate MCP tools with catalog deferral
2. Test MCP server connections with deferral

### Phase 4: eggsact Integration (Future - separate project)

1. Complete eggsact crate
2. Add as feature-gated dependency
3. Register all eggsact tools
4. Test with agent harness

### Phase 5: Search Enhancement (Optional)

1. Implement BM25 ranking
2. Add embeddings index for semantic search (if tool count justifies)
3. Provider-specific optimizations

---

## Key Decisions Needed

1. **Tool naming convention**: Match MCP names (math_eval) or opencode style (calc_eval)?
2. **Default defer behavior**: Should ALL tools be deferred by default, or just MCP/new tools?
3. **Search mode for v1**: Keyword (current) or BM25 upgrade?
4. **Feature gate eggsact**: Always included or opt-in feature?

---

## References

- [Claude Code Tool Search](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)
- [Tool Search with Embeddings Cookbook](https://platform.claude.com/cookbook/tool-use-tool-search-with-embeddings)
- [OpenAI Tool Search](https://platform.openai.com/docs/guides/tools-tool-search)
- [Anthropic Advanced Tool Use](https://www.anthropic.com/engineering/advanced-tool-use)
- nl-clicalc MCP Server: https://github.com/dbowm91/nl-clicalc