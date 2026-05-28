# Review: Batch 8 - Bus, Memory, Shell, and Remaining

**Reviewed**: 2026-05-28
**Files**: architecture/bus.md, architecture/memory.md, architecture/shell_session.md, architecture/tts.md, architecture/upgrade.md, architecture/util.md, architecture/crypto.md, architecture/ide.md

## Summary

Overall the architecture documentation is accurate and well-maintained. Most concrete claims (event counts, buffer sizes, struct definitions, line references) check out against the actual code. The primary issues are: a TTL value mismatch in bus.md (310s in code vs 300s documented), two undocumented modules in util.md (pricing.rs, interner.rs), and a visibility error in crypto.md (EncryptedData is actually `pub`). The memory and shell_session docs are very accurate. The ide.md and tts.md docs correctly capture their respective modules.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | bus.md | 126, 334, 338, 345 | TTL documented as 300 seconds (5 minutes) but code uses 310 seconds (`src/bus/mod.rs:59,126`) | UPDATE: Change 300 to 310, or change "5 minutes" to "~5 minutes" |
| 2 | bus.md | 149 | Code snippet shows `_ => PermissionChoice::DenyOnce` as timeout default, but this is inside the doc's pattern example, not the actual registry code. The actual timeout handling is in the agent loop, not the registry. | CLARIFY: Note this is the calling pattern, not registry behavior |
| 3 | crypto.md | 49 | Doc says "The `EncryptedData` struct is not `pub` (internal to crypto module)" but code at `src/crypto/mod.rs:28` shows `pub struct EncryptedData` | UPDATE: Remove "not pub" claim; struct is public |
| 4 | util.md | entire file | Missing documentation for `pricing.rs` (ModelPricing, PricingService, calculate_cost) which is in `src/util/pricing.rs` and listed in `src/util/mod.rs:4` | ADD: Add pricing.rs section documenting cost calculation |
| 5 | util.md | entire file | Missing documentation for `interner.rs` (StringInterner, tool_interner) which is in `src/util/interner.rs` and listed in `src/util/mod.rs:6` | ADD: Add interner.rs section |
| 6 | util.md | 82-85 | Histogram bounded at 1000 entries (`if vec.len() > 1000 { vec.pop_front(); }` at `src/util/metrics.rs:122-124`) not mentioned | ADD: Note the 1000-entry bound |
| 7 | tts.md | 9-12 | Key Responsibilities says "Platform-specific implementation (macOS-only)" but auto-stop on AgentFinished is a TUI-layer concern, not in the TTS module itself. The doc doesn't mention TUI integration. | CLARIFY: Note that TTS module is standalone; auto-stop and toggle are in TUI layer |
| 8 | memory.md | 147 | Doc says "Max 20 active memories per namespace" but `consolidate_session()` at `src/memory/mod.rs:245` uses `.take(20)` on the scored list, not on the final stored count. The actual namespace count can temporarily exceed 20. | CLARIFY: Note that 20 is a soft limit per consolidation run |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | bus | TTL is 310s not 300s - slight inconsistency but harmless | `src/bus/mod.rs:59,126` | Low |
| 2 | util | Histogram has implicit 1000-entry bound not documented - prevents unbounded memory but isn't documented as a design decision | `src/util/metrics.rs:122-124` | Low |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | util | Document `pricing.rs` with ModelPricing struct and PricingService::calculate_cost formula | Completes util module documentation |
| 2 | util | Document `interner.rs` StringInterner for deduplicating Arc<str> allocations | Documents memory optimization |
| 3 | util | Add note about Histogram 1000-entry bound as a design decision | Prevents confusion about memory usage |
| 4 | tts | Document TUI integration points (toggle via /tts or /voice, auto-stop on AgentFinished, keybindings) separately from TTS module | Separates module scope from TUI integration |
| 5 | memory | Add note that consolidate_session's 20-memory limit is per-run, not a hard namespace cap | Clarifies consolidation behavior |
| 6 | crypto | Consider making EncryptedData `pub(crate)` if internal-only, or document its public API clearly | Clarifies API surface |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | bus.md | "TTL: 300 seconds (5 minutes)" repeated in multiple places | Should be 310 seconds to match code |
| 2 | crypto.md | "The `EncryptedData` struct is not `pub`" | Incorrect - struct is `pub` |

## Verified Claims (Confirmed Correct)

| # | File | Claim | Status |
|---|------|-------|--------|
| 1 | bus.md | AppEvent has 36 variants | CONFIRMED - counted 36 in `src/bus/events.rs:4-150` |
| 2 | bus.md | GlobalEventBus buffer capacity 2048 | CONFIRMED - `broadcast::channel(2048)` at `src/bus/global.rs:13` |
| 3 | bus.md | PermissionRegistry/QuestionRegistry are synchronous (fn, not async fn) | CONFIRMED - all methods are `pub fn` in `src/bus/mod.rs` |
| 4 | bus.md | PermissionRegistry key format `"{tool_call_id}-{tool_name}"` | CONFIRMED - documented in `src/server/routes/permission.rs:65` |
| 5 | bus.md | QuestionRegistry key format is `session_id` only | CONFIRMED - `src/bus/mod.rs:89` |
| 6 | bus.md | SSE handler at `src/server/routes/event.rs` takes no parameters | CONFIRMED per doc |
| 7 | bus.md | PermissionChoice enum has 4 variants | CONFIRMED - AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny |
| 8 | bus.md | `get_pending_permissions_for_session` returns empty list | CONFIRMED - `src/server/routes/permission.rs:62-73` |
| 9 | memory.md | Negation scoring: "don't use" = 5.0, "never use" = 7.0 | CONFIRMED - base 8/10 + negation_modifier -3 |
| 10 | memory.md | Memory struct fields match code | CONFIRMED - `src/memory/mod.rs:14-26` |
| 11 | memory.md | Consolidation threshold >= 8.0 | CONFIRMED - `src/memory/mod.rs:246` |
| 12 | memory.md | flock() for cross-process file synchronization | CONFIRMED - `src/memory/mod.rs:497-516` |
| 13 | shell_session.md | Default terminal size 80x24 | CONFIRMED - `src/shell_session/session.rs:29-30` |
| 14 | shell_session.md | Default shell is bash | CONFIRMED - `src/shell_session/session.rs:28` |
| 15 | shell_session.md | 11 tests covering all operations | CONFIRMED - 11 `#[tokio::test]` functions in `src/shell_session/session.rs` |
| 16 | tts.md | Uses macOS `say` command | CONFIRMED - `src/tts/mod.rs:62` |
| 17 | tts.md | TtsProvider enum has only `None` variant | CONFIRMED - `src/tts/mod.rs:6-9` |
| 18 | tts.md | stop() returns Err on pkill failure | CONFIRMED - `src/tts/mod.rs:98-104` |
| 19 | tts.md | speak() validates non-empty text | CONFIRMED - `src/tts/mod.rs:52-57` |
| 20 | upgrade.md | GitHub API URL `https://api.github.com/repos/anomalyco/codegg/releases/latest` | CONFIRMED - `src/upgrade/mod.rs:25` |
| 21 | upgrade.md | install.sh URL `https://codegg.ai/install.sh` | CONFIRMED - `src/upgrade/mod.rs:73` |
| 22 | upgrade.md | VERSION from CARGO_PKG_VERSION | CONFIRMED - `src/upgrade/mod.rs:5` |
| 23 | upgrade.md | autoupdate config exists but not wired to upgrade module | CONFIRMED - `src/config/schema.rs:34`, upgrade module doesn't read it |
| 24 | util.md | Clipboard uses arboard crate, feature-gated | CONFIRMED - `src/util/clipboard.rs:3` |
| 25 | util.md | fuzzy_match uses strsim levenshtein distance | CONFIRMED - `src/util/fuzzy.rs:1` |
| 26 | util.md | truncate_lines keeps max_lines/2 from start and end | CONFIRMED - `src/util/truncate.rs:6` |
| 27 | crypto.md | AES-256-GCM with 12-byte nonce | CONFIRMED - `src/crypto/mod.rs:8` |
| 28 | crypto.md | Argon2id params m=19456, t=2, p=1, output=32 | CONFIRMED - `src/crypto/mod.rs:35` |
| 29 | crypto.md | v2 prefix "v2:" | CONFIRMED - `src/crypto/mod.rs:10` |
| 30 | crypto.md | Legacy HMAC-SHA256 key derivation | CONFIRMED - `src/crypto/mod.rs:45-58` |
| 31 | ide.md | is_vscode() checks VSCODE_IPC_HOOK, VSCODE_INJECTED_ENVIRONMENT, TERM_PROGRAM | CONFIRMED - `src/ide/mod.rs:80-84` |
| 32 | ide.md | is_jetbrains() checks JETBRAINS_REMOTE, JB_PRODUCT_READINESS, IDEA_INITIAL_DIRECTORY, WEBCLBROWSER_HOST | CONFIRMED - `src/ide/mod.rs:86-91` |
| 33 | ide.md | IdeServer::run_stdio() lines 78-119 | CONFIRMED - `src/mcp/ide_server.rs:78-119` |
| 34 | ide.md | IdeServer::run_socket() lines 121-144 | CONFIRMED - `src/mcp/ide_server.rs:121-144` |
| 35 | ide.md | TempFilesGuard implements Drop for cleanup | CONFIRMED - `src/ide/mod.rs:57-63` |
| 36 | ide.md | register_panic_cleanup uses std::sync::Once | CONFIRMED - `src/ide/mod.rs:65-78` |
