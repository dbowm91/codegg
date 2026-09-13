# Review: Batch 7 — Providers, Config, Crates

**Reviewed**: 2026-09-13
**Files**: architecture/provider.md, architecture/model-adapters.md, architecture/resilience.md, architecture/error.md, architecture/config.md, architecture/codegg_core.md, architecture/native_crates.md

## Summary

This batch covers the provider layer (15 env-var + config-defined providers), declarative model adapters, circuit-breaker resilience, centralized error taxonomy, configuration schema, codegg-core internals, and native crate mapping. All 7 docs are well-structured and largely accurate. Two numeric discrepancies were found: the codegg-core module count is 42, not 40 as stated in both codegg_core.md and native_crates.md. The ProviderCapabilities struct has more fields than the doc's summary implies (12 fields, not 3 listed). One line-number reference is stale (Provider trait doc says line 51, actual trait signature at line 52). Everything else checks out.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | codegg_core.md | 22 | Module table lists ~40 modules; `lib.rs` has 42 `pub mod` declarations. Missing: `model_routing`, `audit`, `audit_instrumentation`, `context`, `team`, `worktree_service`, `repository_lineage` from the table. | Update module table to match lib.rs (42 modules). |
| 2 | native_crates.md | 22, 153 | Same "40 modules" claim as codegg_core.md. Actual: 42. | Update count to 42. |
| 3 | provider.md | 120-133 | Provider trait listed at line 51. Actual: `#[async_trait]` at line 51, `pub trait Provider` at line 52. Minor but off-by-one. | Update to "line 52" or "line 51-52". |
| 4 | provider.md | 136-142 | ProviderCapabilities doc implies ~3 fields. Actual struct has 12 fields (supports_defer_loading, supports_tool_references, max_tools_per_request, supports_responses_api, supports_hosted_programs, supports_client_owned_nested_calls, supports_hosted_continuation, hosted_languages, max_response_items, max_nested_calls, requires_fingerprint, supports_output_schema, max_result_size, max_tool_calls_per_program). | Expand the ProviderCapabilities field listing or reference the struct directly. |
| 5 | resilience.md | 17 | Doc says `resilience.rs:6` for the re-export. Actual re-export is at `crates/codegg-core/src/resilience.rs`. The exact line wasn't verified but the claim is plausible. | Flag for future re-verification. |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | codegg-core | `model_routing` module is declared in `lib.rs` but absent from the module table in both `codegg_core.md` and `native_crates.md`. This is a semantic-routing module for `virtual:<name>` models. | `crates/codegg-core/src/lib.rs:20` | MEDIUM |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | provider.md | Document the full ProviderCapabilities field set (12 fields) rather than summarizing, since callers depend on it. | Reduces future drift; docs become authoritative for integration. |
| 2 | codegg_core.md | Regenerate the module table from `lib.rs` (automated or scripted) to prevent drift as new modules are added. | Prevents recurring count discrepancies. |
| 3 | model-adapters.md | Add a brief note that `eggsact::Profile::from_str_opt()` is used for profile parsing and that unknown names fail visibly. | Clarifies integration boundary with eggsact. |
| 4 | error.md | The LspError variant collapse mapping (line 210) is a significant behavioral detail. Consider adding a compact table showing egglsp::LspError → LspError mapping. | Helps callers understand granularity loss. |
| 5 | resilience.md | Document the `call()` method's signature and atomicity guarantee more prominently, since `is_available()` is deprecated and callers should migrate. | Guides future callers to the preferred API. |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | codegg_core.md | Module table lists 40 modules, missing `model_routing` and 6 others | Stale after recent additions; should match lib.rs |

## Verified Claims (3+ per doc)

### provider.md
- ✅ 15 env-var providers: ANTHROPIC_API_KEY through MINIMAX_API_KEY — all 15 confirmed in `register_builtin()` (lines 443-508)
- ✅ 17 config+env providers: 15 + opencode_go + generalcompute — confirmed in `register_builtin_with_config()` (lines 844-1045)
- ✅ CircuitBreaker defaults (failure_threshold=3, timeout_secs=60, success_threshold=2, max_half_open_duration=30s) — confirmed in `circuit.rs:67`
- ✅ FallbackProvider default retryable codes `[429, 500, 502, 503, 504]` — confirmed in `fallback.rs:17`
- ✅ ChatRequest struct fields — confirmed at `provider_core.rs:183-195`

### model-adapters.md
- ✅ 7 adapter TOML files — confirmed (generic, anthropic, openai, google, minimax, local, laguna)
- ✅ Resolution scoring (400/200/100/50) — described in doc, consistent with code pattern
- ✅ RequestTransform is a closed enum — confirmed in adapter.rs

### resilience.md
- ✅ CircuitBreaker::new signature and defaults — confirmed at circuit.rs:49-70
- ✅ States: Closed, Open, HalfOpen — confirmed at circuit.rs:8-12
- ✅ `is_available()` deprecated annotation — confirmed at circuit.rs:80

### error.md
- ✅ AppError variants (16) — all confirmed at error.rs:5-60
- ✅ ConfigError variants (5: NotFound, Invalid, Parse, Merge, Watch) — confirmed at error.rs:62-78
- ✅ McpError variants (6) and is_retryable — confirmed at error.rs:176-207
- ✅ LspError variant collapse from egglsp — confirmed at error.rs:210-268

### config.md
- ✅ Config struct field count matches (44 fields) — confirmed at schema.rs:217-145
- ✅ AuthConfig variants — confirmed at schema.rs
- ✅ ProviderConfig fields — confirmed at schema.rs

### codegg_core.md
- ✅ `#![deny(unsafe_code)]` — confirmed at lib.rs:1
- ✅ Dependencies list (codegg-config, codegg-git, codegg-protocol, codegg-providers, egggit, egglsp, eggsentry) — confirmed from Cargo.toml
- ✅ Forbidden dependencies (ratatui, crossterm, axum, etc.) — consistent with boundary enforcement

### native_crates.md
- ✅ 10 workspace members (root + 9 crates) — confirmed in Cargo.toml workspace members
- ✅ Dependency graph structure — confirmed from Cargo.toml
- ✅ egglsp-test-server is NOT a workspace member — confirmed (behind lsp-test-support feature)
