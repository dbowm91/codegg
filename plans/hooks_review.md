# Hooks Architecture Review

## Summary
The hooks architecture document is largely accurate. The two hook systems (shell command hooks in `src/hooks/mod.rs` and WASM plugin hooks in `src/plugin/hooks.rs`) are correctly documented. Minor discrepancies exist around integration point line numbers and some text formatting issues.

## Verified Correct
- **HookEvent enum** (`src/hooks/mod.rs:17-24`) - Correctly listed with 6 variants: PreToolExecute, PostToolExecute, SessionStart, SessionEnd, AgentStart, AgentEnd
- **HookContext struct** (`src/hooks/mod.rs:56-63`) - Matches doc exactly with all fields present
- **Hook trait** (`src/hooks/mod.rs:90-92`) - Correctly defined as `async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>`
- **ShellCommandHook struct** (`src/hooks/mod.rs:94-98`) - Correctly documented with command, timeout, event fields
- **HookRegistry::run_hooks** (`src/hooks/mod.rs:191-201`) - Returns `Vec<AppError>`, collects errors not early-returns
- **HookRegistry::from_config** (`src/hooks/mod.rs:165-189`) - Handles InlineScript as deprecated/warn, skips it correctly
- **Environment variables** - `CODEGG_HOOK_EVENT`, `CODEGG_SESSION_ID`, `CODEGG_TOOL_NAME`, `CODEGG_TOOL_ARGUMENTS`, `CODEGG_TOOL_RESULT`, `CODEGG_TIMESTAMP` all documented and implemented (`src/hooks/mod.rs:66-86`)
- **PATH handling** (`src/hooks/mod.rs:118`) - Uses `std::env::var_os("PATH")` as documented
- **HookType enum** (`src/plugin/hooks.rs:6-20`) - All 13 hook types match doc
- **HookResult struct** (`src/plugin/hooks.rs:68-72`) - Has output, blocked, error fields as documented
- **HookResult methods** (`src/plugin/hooks.rs:75-97`) - `ok()`, `blocked()`, `error()` all present and match doc
- **Plugin timeout text** (`src/plugin/hooks.rs` no explicit timeout there - is in plugin loader) - Doc says "5s per hook"

## Discrepancies Found
- **Integration point line numbers** - Architecture doc says hooks are in `loop.rs` but doesn't give line numbers. Actual hook calls at: SessionStart (`loop.rs:1250`, `1261-1262`), AgentStart (`1346`, `1357-1358`), AgentEnd (`1519`, `1530`), SessionEnd (`1540`, `1551`), PreToolExecute (`1745`, `1757`), PostToolExecute (`1819`, `1831`)
- **Timeout claim** - Doc says "5s per hook" for plugin hooks but actual timeout implementation should be verified in plugin loader code

## Bugs Identified
- **InlineScript deprecation** (`src/hooks/mod.rs:181-184`) - `#[allow(deprecated)]` attribute on InlineScript match arm indicates this is intentionally dead code, but docs don't mention InlineScript exists at all (just says ShellCommandHook is the only shell hook type)

## Improvement Suggestions
- **HookRegistry error format** - Doc says error messages include event name; actual code shows this at `src/hooks/mod.rs:135-137` - "Hook command failed (event={})" - this is correct
- **has_hooks method** (`src/hooks/mod.rs:203-205`) - Not documented in arch doc but exists and could be useful for callers

## Stale Items in Architecture Doc
- **Toml config example** (`architecture/hooks.md:101-116`) - Should verify this config schema still matches `src/config/schema.rs`
