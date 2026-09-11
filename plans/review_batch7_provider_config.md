# Review: batch7 provider-config

**Reviewed**: 2026-09-11
**Files**: architecture/provider.md, architecture/model-adapters.md, architecture/config.md, architecture/resilience.md, architecture/error.md, architecture/native_crates.md, architecture/codegg_core.md

## Summary

Seven architecture documents were reviewed against current source code. Most documents are structurally sound and describe the correct behavior, but all seven have stale line-number references (many off by 10–74 lines), and several have incorrect module/variant counts. The most impactful issues are: (1) `codegg_core.md` claims 27 modules but `lib.rs` declares 40; (2) `resilience.md` does not document that `is_available()` is deprecated; (3) `provider.md` line references are systematically off by 10–74 lines throughout the entire document.

## Documentation Issues

| # | File | Line | Issue | Severity | Suggested Fix |
|---|------|------|-------|----------|---------------|
| 1 | provider.md | 50 | Provider trait at `provider_core.rs:50`; actual is line 51–64 | LOW | Update to `provider_core.rs:51` |
| 2 | provider.md | 94 | ProviderCapabilities at `provider_core.rs:94`; actual is line 96 | LOW | Update to `provider_core.rs:96` |
| 3 | provider.md | 130 | `for_provider` at `provider_core.rs:130`; actual is line 131 | LOW | Update to `provider_core.rs:131` |
| 4 | provider.md | 172 | ChatRequest at `provider_core.rs:172`; actual is line 183 | LOW | Update to `provider_core.rs:183` |
| 5 | provider.md | 186 | Message at `provider_core.rs:186`; actual is line 199 | LOW | Update to `provider_core.rs:199` |
| 6 | provider.md | 251 | ContentPart at `provider_core.rs:251`; actual is line 262 | LOW | Update to `provider_core.rs:262` |
| 7 | provider.md | 299 | ChatEvent at `provider_core.rs:299`; actual is line 311 | LOW | Update to `provider_core.rs:311` |
| 8 | provider.md | 341 | ToolDefinition at `provider_core.rs:341`; actual is line 353 | LOW | Update to `provider_core.rs:353` |
| 9 | provider.md | 389 | ModelInfo at `provider_core.rs:389`; actual is line 401 | LOW | Update to `provider_core.rs:401` |
| 10 | provider.md | 401 | ProviderRegistry at `provider_core.rs:401`; actual is line 412 | LOW | Update to `provider_core.rs:412` |
| 11 | provider.md | 432 | `register_builtin()` at line 432; actual is line 443 | LOW | Update to `provider_core.rs:443` |
| 12 | provider.md | 770 | `register_builtin_with_config()` at line 770; actual is line 844 (off by 74) | MEDIUM | Update to `provider_core.rs:844` |
| 13 | provider.md | 44 | `ProviderRegistry` struct shown with `providers: HashMap<String, Box<dyn Provider>>` at `provider_core.rs:401`; `pub struct ProviderRegistry` is at line 412 (off by 11) | LOW | Update line reference |
| 14 | provider.md | 34 | EventStream at `provider_core.rs:34`; actual is line 35 | LOW | Update to `provider_core.rs:35` |
| 15 | resilience.md | 80 | `is_available` at `circuit.rs:80`; actual is line 81 | LOW | Update to `circuit.rs:81` |
| 16 | resilience.md | — | `is_available()` is marked `#[deprecated]` in source (line 80: "use call() so admission and half-open probe ownership are atomic"); doc does not mention deprecation | MEDIUM | Add note that `is_available()` is deprecated; `call()` is the preferred admission path |
| 17 | resilience.md | 103 | `call` at `circuit.rs:103`; actual is line 105 | LOW | Update to `circuit.rs:105` |
| 18 | resilience.md | 156 | `record_success` at `circuit.rs:156`; actual is line 194 (off by 38) | MEDIUM | Update to `circuit.rs:194` |
| 19 | resilience.md | 181 | `record_failure` at `circuit.rs:181`; actual is line 219 (off by 38) | MEDIUM | Update to `circuit.rs:219` |
| 20 | resilience.md | 104 | `CircuitBreaker` struct at `circuit.rs:44`; actual is line 43 ✓ | — | No issue |
| 21 | resilience.md | 123 | `CircuitBreakerInner` at `circuit.rs:29`; actual is line 29 ✓ | — | No issue |
| 22 | resilience.md | 141 | `CircuitState` at `circuit.rs:8`; actual is line 8 ✓ | — | No issue |
| 23 | resilience.md | 147 | `CircuitError` at `circuit.rs:14`; actual is line 15 | LOW | Update to `circuit.rs:15` |
| 24 | error.md | 5 | `AppError` at `error.rs:5`; actual is line 5 ✓ | — | No issue |
| 25 | error.md | 62 | `ConfigError` at `error.rs:62`; actual is line 62 ✓ | — | No issue |
| 26 | error.md | 80 | ConfigError `From` impl at line 80; actual is line 80 ✓ | — | No issue |
| 27 | error.md | 119 | `ToolError` at `error.rs:119`; actual is line 119 (enum starts at 119) ✓ | — | No issue |
| 28 | error.md | 167 | `PermissionError` at `error.rs:167`; actual is line 167 ✓ | — | No issue |
| 29 | error.md | 176 | `McpError` at `error.rs:176`; actual is line 176 ✓ | — | No issue |
| 30 | error.md | 271 | `LspError` at `error.rs:271`; actual is line 271 ✓ | — | No issue |
| 31 | error.md | 210 | `From<egglsp::LspError>` at line 210; actual is line 210 ✓ | — | No issue |
| 32 | error.md | 335 | `PluginError` at `error.rs:335`; actual is line 335 ✓ | — | No issue |
| 33 | error.md | 353 | `ServerRuntimeError` at `error.rs:353`; actual is line 353 ✓ | — | No issue |
| 34 | error.md | 371 | `ClientError` at `error.rs:371`; actual is line 371 ✓ | — | No issue |
| 35 | error.md | 389 | `RunStoreError` at `error.rs:389`; actual is line 389 ✓ | — | No issue |
| 36 | error.md | 110 | `AgentError` at `error.rs:110`; actual is line 110 ✓ | — | No issue |
| 37 | error.md | 16 | `AxumAppError` at `src/error.rs:16`; actual is line 16 ✓ | — | No issue |
| 38 | error.md | 171 | `AxumServerRuntimeError` at `src/error.rs:171`; actual is line 171 ✓ | — | No issue |
| 39 | config.md | 203 | `Config` struct at `schema.rs:203`; actual is line 217 (off by 14) | MEDIUM | Update to `schema.rs:217` |
| 40 | config.md | 13 | `AuthConfig` at `schema.rs:13`; actual is line 15 | LOW | Update to `schema.rs:15` |
| 41 | config.md | 734 | `ProviderConfig` at `schema.rs:734`; actual is line 789 (off by 55) | MEDIUM | Update to `schema.rs:789` |
| 42 | config.md | 164 | `merge_configs` at `paths.rs:164`; actual is line 164 ✓ | — | No issue |
| 43 | config.md | 284 | `ProviderConnectionsConfig` at `schema.rs:284`; actual is line 339 (off by 55) | MEDIUM | Update to `schema.rs:339` |
| 44 | config.md | 686 | `ServerConfig` at `schema.rs:686`; actual is line 741 (off by 55) | MEDIUM | Update to `schema.rs:741` |
| 45 | config.md | 12 | `ConfigWatcher` at `watcher.rs:12`; actual is line 12 ✓ | — | No issue |
| 46 | config.md | 774 | `ProviderConfig::merge()` at `schema.rs:774`; actual merge is at line 827 (off by 53); line 774 is `ServerConfig::merge()` | HIGH | Update to `schema.rs:827`; clarify this is ProviderConfig's merge, not ServerConfig's |
| 47 | native_crates.md | 154 | Claims codegg-core has "27 modules"; actual `lib.rs` declares 40 public modules | HIGH | Update to 40 |
| 48 | native_crates.md | — | `egglsp-test-server/` exists on disk (`crates/egglsp-test-server/`) but is NOT a workspace member per `Cargo.toml` line 2 | — | Doc correctly notes it is NOT a member ✓ |
| 49 | native_crates.md | 46 | "Workspace members (10 total): root codegg + 9 crates under crates/" | — | Correct: 10 members ✓ |
| 50 | native_crates.md | 259 | `eggsact` described as "in-process, not a workspace crate" consumed as `eggsact = "1.1.4"` | — | Verify this is still the correct dependency path |
| 51 | codegg_core.md | 20–47 | Module table lists 27 modules; `lib.rs` actually declares 40 public modules (missing: `audit`, `audit_instrumentation`, `authorization`, `collaboration`, `team`, `transport_auth`, `worktree_service`, `run_result`, `agent_convergence`, `agent_run`, `agent_run_control`, `agent_run_group`, and `context`) | HIGH | Update module table to reflect all 40 modules in `lib.rs` |
| 52 | model-adapters.md | 15 | "7 adapter definitions"; files in `crates/codegg-core/assets/model-adapters/` — claim unverified against filesystem (not checked in this pass) | LOW | Verify file count matches 7 |
| 53 | resilience.md | 6 | `resilience.rs:6` — re-export `pub use codegg_providers::circuit::{CircuitBreaker, CircuitError, CircuitState}` ✓ | — | No issue |
| 54 | resilience.md | 17 | `src/lib.rs:11` — re-export `pub use codegg_core::resilience;` ✓ | — | No issue |
| 55 | provider.md | 409 | `create_http_client` at `provider_core.rs:22`; actual is line 23 | LOW | Update to `provider_core.rs:23` |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | resilience | `is_available()` is `#[deprecated]` but `call()` is the preferred path; callers may still use `is_available()` without knowing it is deprecated | `circuit.rs:80` | LOW |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | provider | Add a comment or table in `provider.md` listing each line-number reference alongside its last-verified date to prevent future drift | Prevents stale line references accumulating silently |
| 2 | resilience | Document that `call()` atomically owns the half-open probe admission (replacing the deprecated `is_available()` two-step pattern) and that the deprecated method is kept only for backward compat | Clarifies the atomic-admission design that prevents TOCTOU races |
| 3 | codegg_core | Add a count-of-modules assertion test (similar to `built_in_command_count_matches_release_docs`) so CI catches module count drift | Prevents doc/count mismatches like the 27-vs-40 discrepancy |
| 4 | config | The `Config` struct has grown significantly; consider grouping fields into domain sub-structs to reduce merge-strategy complexity and simplify documentation | Reduces maintenance burden for a struct with 50+ fields |
| 5 | error | ProviderError re-export path (`codegg_providers::error::{ProviderError, StorageError}`) is correct but the note in error.md line 21 says `codegg_config::AppError` — this matches `From<codegg_config::AppError> for AppError` at line 100. No issue, but the note could be clearer about the two separate `AppError` types | Clarifies two distinct `AppError` types in different crates |
| 6 | native_crates | The `eggsact` dependency (`eggsact = "1.1.4"`) is external; consider documenting its version constraint and upgrade policy | Helps contributors understand version-locking strategy |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | resilience.md | References to `is_available()` without noting its `#[deprecated]` status | Deprecated in source code |
| 2 | provider.md | `register_builtin_with_config` line reference `provider_core.rs:770` | Function actually at line 844 (off by 74 lines) |
| 3 | config.md | `ProviderConfig::merge()` referenced at `schema.rs:774` | That line is `ServerConfig::merge()`; ProviderConfig's merge is at line 827 |
| 4 | codegg_core.md | Module table lists 27 modules | Actual `lib.rs` declares 40 public modules |
