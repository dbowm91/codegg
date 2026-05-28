# Stale Item Pruning Pass

**Date**: 2026-05-28

## Modules Without Architecture Docs

| Module | Path | Notes |
|--------|------|-------|
| (none) | | All top-level `src/` modules have corresponding architecture documents |

## Architecture Docs Without Corresponding Modules

| Doc | References | Status |
|-----|-----------|--------|
| `overview.md` | Top-level index | Intentional - overview/index document, not a module doc |
| `compaction.md` | `src/agent/compaction.rs` | Valid - documents a distinct submodule within `agent/` |
| `error.md` | `src/error.rs` | Valid - documents a single-file module |
| `exec.md` | `src/exec.rs` | Valid - documents a single-file module |

## Entirely Stale Documents

| Document | Reason |
|----------|--------|
| (none) | No architecture documents are entirely stale |

## Sections Marked for Removal/Update

| Document | Section | Reason |
|----------|---------|--------|
| `overview.md:106` | Provider auto-registration claim | Says "Auto-registered: codegg_zen only" but `register_builtin()` registers 15 providers: anthropic, openai, google, openrouter, codegg_zen, mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, minimax |
| `provider.md:205` | `register_builtin_with_config()` description | Says "Registers only `codegg_go` as auto-registered via `register_builtin()`" but `codegg_go` is NOT in `register_builtin()` at all - it's only in `register_builtin_with_config()` via `register_env_fallback_provider()` |
| `provider.md:208` | Key distinction note | Says "Only `codegg_go` is auto-registered via `register_builtin()`" - same stale claim as above |
| `provider.md:624` | Auto-Registration Summary | Says "Only `codegg_go` is auto-registered" - same stale claim |
| `provider.md:641` | ProviderError reference path | References `src/error/mod.rs` but the file is `src/error.rs` (single file, not a directory) |
| `tool.md:327` | ToolError reference path | References `src/error/mod.rs` but the file is `src/error.rs` |
| `tui.md:31` | App struct line count | Says "(6003 lines)" but actual count is 5995 lines (8 lines off) |

## Summary

**No modules are missing architecture docs.** The `overview.md`, `compaction.md`, `error.md`, and `exec.md` files are all valid despite not having a 1:1 top-level `src/` module mapping.

**No documents are entirely stale.** The main issues are:

1. **Stale provider auto-registration claims** (3 locations in `provider.md`, 1 in `overview.md`): Both docs incorrectly state which providers are auto-registered. The actual `register_builtin()` function registers 15 providers via env var checks, while `codegg_go` is only registered through `register_builtin_with_config()`.

2. **Stale file path references** (2 locations): `provider.md` and `tool.md` reference `src/error/mod.rs` but the error module is a single file at `src/error.rs`.

3. **Minor line count drift** (1 location): `tui.md` claims 6003 lines for `app/mod.rs` but actual is 5995.
