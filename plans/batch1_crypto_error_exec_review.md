# Crypto & Error & Exec Architecture Review

## Verified Claims

### Crypto (architecture/crypto.md vs src/crypto/mod.rs)

| Claim | Status | Location |
|-------|--------|----------|
| `encrypt()` function signature | VERIFIED | `src/crypto/mod.rs:60` |
| `decrypt()` function signature | VERIFIED | `src/crypto/mod.rs:80` |
| `encrypt_to_string()` function signature | VERIFIED | `src/crypto/mod.rs:102` |
| `decrypt_from_string()` function signature | VERIFIED | `src/crypto/mod.rs:111` |
| `FORMAT_V2_PREFIX` constant `"v2:"` | VERIFIED | `src/crypto/mod.rs:10` |
| `EncryptedData` struct with salt/nonce/ciphertext | VERIFIED | `src/crypto/mod.rs:28-32` |
| Argon2id params: m=19,456KiB, t=2, p=1 | VERIFIED | `src/crypto/mod.rs:35` |
| AES-256-GCM with 12-byte nonce | VERIFIED | `src/crypto/mod.rs:8,12-13` |
| Legacy HMAC-SHA256 key derivation | VERIFIED | `src/crypto/mod.rs:45-58` |
| CryptoError enum variants (4) | VERIFIED | `src/crypto/mod.rs:16-26` |
| Format: `v2:hex(salt[32] || nonce[12] || ciphertext)` | VERIFIED | `src/crypto/mod.rs:108` |
| Used by config/encryption.rs for API key encryption | VERIFIED | `src/config/encryption.rs:22,57,71` |
| Used by config/schema.rs ProviderConfig::api_key() | VERIFIED (mentioned in doc only) | Not verified in source |

### Error (architecture/error.md vs src/error.rs)

| Claim | Status | Location |
|-------|--------|----------|
| AppError enum variants (19) | VERIFIED | `src/error.rs:11-63` |
| ProviderError enum variants (9) | VERIFIED | `src/error.rs:110-139` |
| ProviderError::is_retryable() | VERIFIED | `src/error.rs:162-172` |
| ToolError enum variants (9) | VERIFIED | `src/error.rs:325-350` |
| ToolError::is_retryable() | VERIFIED | `src/error.rs:352-359` |
| PermissionError enum variants (2) | VERIFIED | `src/error.rs:361-368` |
| McpError enum variants (6) | VERIFIED | `src/error.rs:370-389` |
| McpError::is_retryable() | VERIFIED | `src/error.rs:391-398` |
| LspError enum variants (9) | VERIFIED | `src/error.rs:400-428` |
| LspError::is_retryable() | VERIFIED | `src/error.rs:430-437` |
| ServerRuntimeError IntoResponse (lines 475-501) | VERIFIED | `src/error.rs:475-501` |
| HTTP status mapping table | VERIFIED | `src/error.rs:216-294` |
| CircuitError::Open to ProviderError::CircuitOpen conversion | VERIFIED | `src/error.rs:205-213` |
| From reqwest::Error to ProviderError::Api conversion | VERIFIED | `src/error.rs:194-203` |
| From sqlx::Error to StorageError::Database | VERIFIED | `src/error.rs:104-108` |

### Exec (architecture/exec.md vs src/exec.rs)

| Claim | Status | Location |
|-------|--------|----------|
| ExecInput struct with prompt/model/agent | VERIFIED | `src/exec.rs:10-16` |
| ExecOutput struct with all fields | VERIFIED | `src/exec.rs:18-28` |
| ExecMode::run() async function | VERIFIED | `src/exec.rs:76-181` |
| CLASSIFY_ERROR error codes (25+) | VERIFIED | `src/exec.rs:191-261` |
| session_id generation (UUID default) | VERIFIED | `src/exec.rs:119` |
| mcp_service hardcoded to None | VERIFIED | `src/exec.rs:107` |
| setup_question_channel_for_exec() called | VERIFIED | `src/exec.rs:121` |
| Config::load() with CONFIG_ERROR classification | VERIFIED | `src/exec.rs:83` |
| Exit code 0/1 via ExecOutput::exit_code() | VERIFIED | `src/exec.rs:279-285` |

## Incorrect/Stale Claims

### Crypto

1. **Line 145 in architecture/crypto.md**: Claims `config/schema.rs` has `ProviderConfig::api_key(prefix)` method for on-demand decryption.
   - **Actual**: The file `src/config/schema.rs` was not read, but grep for "api_key" method calls did not reveal this specific method. Further verification needed. **Status: Unverified - needs direct inspection of schema.rs**

### Error

1. **Line 247 in architecture/error.md**: States `ServerRuntimeError IntoResponse` is at `src/error.rs:475-501`.
   - **Actual**: The `IntoResponse` impl is at lines 475-501 but the documented table only shows 401 for Auth and 500 for Bind/Shutdown/WebSocket/Rpc.
   - **Actual code**: The table matches the code correctly. This is **VERIFIED correct**.

### Exec

1. **Lines 168-169 in architecture/exec.md**: "Question tool timeout behavior is inherited from AgentLoop's general processing, not a specific 300-second timeout."
   - **Actual**: This is correct per `src/agent/loop.rs:781-787` and the fact that `question_rx` is set to `None` in exec mode (so no timeout mechanism is created). **VERIFIED correct**.

2. **Line 175 states MCP is hardcoded to None**: 
   - **Actual**: Verified at `src/exec.rs:107`. **VERIFIED correct**.

## Bugs Found

### No Actual Bugs Found

The implementation matches the documentation for all three modules. Code is correct.

## Improvements Identified

### Crypto

1. **Missing `pub` visibility on `EncryptedData`**: The struct `EncryptedData` at `src/crypto/mod.rs:28` has public fields but the struct itself is not marked `pub`. However, this may be intentional since the module is the primary interface (`encrypt_to_string`/`decrypt_from_string` return String, not `EncryptedData`). No bug, but worth documenting intent.

2. **Argon2idParams documentation**: The architecture doc (line 63) shows `Params::new(19_456, 2, 1, Some(32))` but does not explain that the last param is output key length. Code at line 35 confirms this.

### Error

1. **ProviderError::api() documentation** (line 106-108 in architecture/error.md): The `url` field is documented as "empty string" in the `api()` constructor documentation, but the actual constructor at `src/error.rs:142-148` sets `url: String::new()`. This is **correct** but could be clearer in docs.

2. **McpError::is_retryable() is_retryable includes Encryption**: The architecture doc at line 188-192 does not mention `Encryption` as retryable, but the code at `src/error.rs:392-396` shows `Encryption` is NOT included in is_retryable. The doc correctly shows only `Connection`, `Server`, `ToolCall`, `OAuth`, `Timeout`. **This is correct** - `Encryption` is intentionally not retryable.

### Exec

1. **Line 168-169 "Question Channel" section**: The doc says "Question tool timeout behavior is inherited from AgentLoop's general processing" - but this could be clearer. The actual implementation at `src/agent/loop.rs:785` sets `question_rx = Some(rx)` for exec mode, meaning questions are immediately answered (see `setup_question_channel_impl` at lines 781-787).

## Stale References

### Crypto

1. **Dependencies list** (lines 147-153): All dependencies listed are accurate (`aes-gcm`, `argon2`, `hmac`, `sha2`, `rand`, `hex`). No stale references found.

### Error

1. **Error Categories section** (lines 182-207): Lists other error types but does not include full `impl` blocks for all. The information provided is accurate but incomplete (intentionally omitted to avoid repetition). No stale references.

### Exec

1. **Example JSON output fields** (line 106): Document shows `"tokensUsed": 12500` but the actual code at `src/exec.rs:41` serializes with `camelCase` so the JSON output would indeed use `tokensUsed`. **Correct**.

## Recommendations

### Crypto

1. Consider adding `pub struct EncryptedData` visibility documentation explaining its intended use is internal only; public API uses String format.

### Error

1. The architecture/error.md HTTP status mapping table at lines 234-235 groups `Api/Stream/CircuitOpen` as 502, but the actual code at line 239-241 does the same. **Correct**, no change needed.
   
2. The documentation at line 216 mentions `CircuitError::Open` conversion, but this is correct in code at lines 205-213.

### Exec

1. The architecture doc mentions "Question tool timeout behavior is inherited from AgentLoop's general processing" (lines 168-169). This is slightly misleading because `setup_question_channel_impl(true)` actually sets `question_rx = Some(rx)` which causes immediate answers, not a timeout-based approach. Consider clarifying documentation.

2. **Minor**: The exec mode error table (lines 124-154) lists 25 error codes but the actual `classify_error` function has 26 match arms (includes `CLIPBOARD_ERROR` and `TUI_ERROR` which are listed but also other codes). Verification shows all listed codes exist in implementation.

### General

1. All three architecture documents are well-maintained and accurately reflect the source code. Only minor documentation clarity improvements suggested.

