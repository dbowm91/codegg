# Execution & Security Modules Architecture Review

## Executive Summary

Reviewed four architecture documents against source code:
- `architecture/exec.md` (180 lines)
- `architecture/security.md` (206 lines)
- `architecture/permission.md` (360 lines)
- `architecture/crypto.md` (158 lines)

**Overall Assessment**: Documentation is largely accurate with minor discrepancies in descriptions and formatting details.

---

## 1. EXEC Module (`src/exec.rs`)

### Verification Results

| Item | Status | Notes |
|------|--------|-------|
| `ExecInput` struct | ✅ Verified | Fields match at lines 10-16 |
| `ExecOutput` struct | ✅ Verified | Fields match at lines 18-28 |
| `classify_error()` error codes | ✅ Verified | All error codes match actual AppError variants |
| `mcp_service = None` | ✅ Verified | Line 107 |
| Session ID generation | ✅ Verified | UUID generation at line 119 |
| Config loading error handling | ✅ Verified | Line 83 properly maps errors |

### Stale Items

1. **Missing error codes in documentation** (minor):
   - `exec.md:125-154` lists error codes table but doesn't mention `INTERNAL_ERROR` used in `print_output()` at `src/exec.rs:263`

2. **Description imprecision at `exec.md:169`**:
   - States "Question tool timeout behavior is inherited from AgentLoop's general processing"
   - Actual: `loop_instance.setup_question_channel()` is called (line 121), but no specific timeout is documented

### Potential Issues in Source

1. **Error message formatting** (`src/exec.rs:176`):
   ```rust
   Ok(ExecOutput::error(format!("{}: {} ({}ms)", msg, e, duration_ms), code))
   ```
   Error includes both `msg` (short description) AND `e` (full error), which may be redundant. For example: `"Permission denied: Permission denied..."`.

2. **Missing `ExecMode::new()` documentation**:
   - `exec.md:165-166` mentions `ExecMode::new()` accepts `session_id`
   - Actual constructor takes `(quiet: bool, json_output: bool, session_id: Option<String>)` - three parameters

---

## 2. SECURITY Module (`src/security/`)

### Verification Results

| Item | Status | Notes |
|------|--------|-------|
| `is_internal_ip()` | ✅ Verified | `src/security/ssrf.rs:4-37` |
| `ipv6_segments_to_ipv4()` | ✅ Verified | `src/security/ssrf.rs:39-65` |
| `validate_host_ip()` | ✅ Verified | `src/security/ssrf.rs:67-94` |
| `revalidate_dns()` | ✅ Verified | `src/security/ssrf.rs:96-121` |
| `validate_url_host()` | ✅ Verified | `src/security/ssrf.rs:123-145` |
| `validate_path_safety()` | ✅ Verified | `src/security/sandbox.rs:259-282` |
| `SandboxConfig` struct | ✅ Verified | `src/security/sandbox.rs:6-11` |
| `get_default_allowed_paths()` | ✅ Verified | `src/security/sandbox.rs:284-308` |
| `get_sensitive_paths()` | ✅ Verified | `src/security/sandbox.rs:310-321` |

### Stale Items

1. **Landlock availability check at `security.md:120`**:
   - Documents `is_available()  // Linux 5.13+ with landlock support`
   - Actual code (`src/security/sandbox.rs:33-42`) only checks for `/sys/kernel/security/landlock` existence or `landlock` in `/proc/filesystems` - no kernel version check

2. **IPv6 unique local description at `security.md:197`**:
   - States `fc00::/8` (IPv6 unique local: fd00::/8 only)
   - Code at `src/security/ssrf.rs:25` uses `(segments[0] & 0xfe00) == 0xfc00` which covers entire `fc00::/7` range (fc00::/8 and fd00::/8). The comment is slightly misleading

3. **IPv4-mapped IPv6 handling in `is_internal_ip()`**:
   - At `src/security/ssrf.rs:22-24`, IPv4-mapped addresses (::ffff:x.x.x.x) are detected via `ipv6_segments_to_ipv4()`, but there's also an explicit check for IPv4-mapped at line 34 that handles pure IPv6 with all-zero segments
   - This dual handling could be simplified

### Potential Issues in Source

1. **IPv6 site-local address check at `ssrf.rs:203-206`**:
   - Test `test_is_internal_ip_site_local` tests `fe80::/10` (link-local) but comment says "site-local"
   - Actual check at line 21 uses `ipv6.is_unicast_link_local()` which correctly handles `fe80::/10`
   - This is a test comment issue only, not a bug

2. **Missing test coverage for `validate_path_safety` symlink check**:
   - The symlink check happens BEFORE canonicalization (`src/security/sandbox.rs:260-265`)
   - If a symlink points outside allowed paths, it will be rejected even if the target is within allowed paths
   - This may be intentional but is not documented

3. **Static cache in `get_canonical_paths()` at `src/security/sandbox.rs:237`**:
   - Uses a `static Mutex<Option<HashMap<...>>>` with interior mutability
   - Cache is never cleared after initialization
   - If allowed paths change during runtime, cache won't reflect changes

---

## 3. PERMISSION Module (`src/permission/`)

### Verification Results

| Item | Status | Notes |
|------|--------|-------|
| `PermissionLevel` enum | ✅ Verified | `src/permission/mod.rs:89-95` |
| `PermissionResult` enum | ✅ Verified | `src/permission/mod.rs:107-112` |
| `PermissionRequest` struct | ✅ Verified | `src/permission/mod.rs:114-119` |
| `PermissionChoice` enum | ✅ Verified | `src/permission/mod.rs:128-134` |
| `PermissionResponse` struct | ⚠️ Misleading | Defined at `mod.rs:1141-1145` but unused internally |
| `ToolRule` struct | ✅ Verified | `src/permission/mod.rs:152-158` |
| `PathRule` struct | ✅ Verified | `src/permission/mod.rs:199-203` |
| `PermissionRuleset` struct | ✅ Verified | `src/permission/mod.rs:205-210` |
| `PersistentDecision` struct | ✅ Verified | `src/permission/mod.rs:222-230` |
| `PermissionStore` struct | ✅ Verified | `src/permission/mod.rs:232-235` |
| `PermissionChecker` struct | ✅ Verified | `src/permission/mod.rs:392-402` |
| `DoomLoopDetector` struct | ✅ Verified | `src/permission/mod.rs:1161-1166` |
| `check_external_directory()` | ✅ Verified | `src/permission/mod.rs:1237-1248` |
| `PermissionRegistry` | ✅ Verified | `src/bus/mod.rs:11-74` |

### Stale Items

1. **`permission.md:61-71` PermissionResponse documentation**:
   - Documents `PermissionResponse { level, persist }` as "Internal permission response type"
   - Actual `PermissionResponse` at `src/permission/mod.rs:1141-1145` is defined but NOT used by any code in the permission module
   - There is a different `PermissionResponse` in `src/server/routes/permission.rs:7` and `src/protocol/tui.rs:26`
   - This internal type appears vestigial

2. **Mode definitions at `permission.md:196-202`**:
   - Table shows built-in modes with tool lists
   - `src/permission/modes.rs:107-181` shows actual implementations:
     - `review` mode has `lsp` in allowed_tools (line 121), not listed in permission.md
     - `docs` mode includes `write` tool (line 171), not listed in permission.md
   - These discrepancies indicate the table is incomplete

3. **PermissionRegistry TTL at `permission.md:340`**:
   - States "TTL of 300s for entries"
   - Actual code at `src/bus/mod.rs:58-62` uses 300 seconds - **correct**

### Potential Issues in Source

1. **`check_external_directory()` unused function** (`src/permission/mod.rs:1237-1248`):
   - Marked `#[allow(dead_code)]`
   - Not used anywhere in codebase
   - Could be removed or should be integrated

2. **IPv4-mapped IPv6 check in `is_internal_ip()`** (`src/security/ssrf.rs:22-24` and 27-34):
   - The function checks IPv4-mapped addresses twice:
     - First via `ipv6_segments_to_ipv4()` at lines 22-24
     - Then via explicit segment check at lines 27-34 for pure IPv6 with all zeros
   - The explicit segment check (lines 27-34) checks for `::` (all zeros), not IPv4-mapped
   - The IPv4-mapped check correctly uses `ipv6_segments_to_ipv4()` for `::ffff:x.x.x.x`
   - This is correct but the dual handling is confusing

3. **PermissionChecker field types** (`src/permission/mod.rs:392-402`):
   - Fields use `Arc<RwLock<...>>` for shared mutable state
   - `canonicalized_*_tool_rules` are `Vec<CanonicalizedToolRule>`, not `Arc` - potential borrow conflict with `with_session_rules()` and `with_agent_rules()` that reassign these fields

4. **Session filtering limitation** (documented at `permission.md:342-354`):
   - This is correctly documented as an architectural limitation
   - The issue exists in `bus/mod.rs:22` where `register()` uses `perm_id` without session context
   - However, `PermissionPending` event does carry `session_id` - this information is just not preserved in the registry key

---

## 4. CRYPTO Module (`src/crypto/mod.rs`)

### Verification Results

| Item | Status | Notes |
|------|--------|-------|
| `encrypt()` | ✅ Verified | Lines 60-78 |
| `decrypt()` | ✅ Verified | Lines 80-100 |
| `encrypt_to_string()` | ✅ Verified | Lines 102-109 |
| `decrypt_from_string()` | ✅ Verified | Lines 111-120 |
| `EncryptedData` struct | ✅ Verified | Lines 28-32 |
| `CryptoError` enum | ✅ Verified | Lines 16-26 |
| `derive_key_argon2id()` | ✅ Verified | Lines 34-43 |
| `derive_key_legacy()` | ✅ Verified | Lines 45-58 |
| `FORMAT_V2_PREFIX` | ✅ Verified | Line 10: `"v2:"` |

### Stale Items

1. **CryptoError variants at `crypto.md:102-111`**:
   - Documents 5 variants: `EncryptionFailed`, `DecryptionFailed`, `InvalidFormat`, `KeyDerivationFailed`
   - Actual code at `src/crypto/mod.rs:16-26` has exactly these 5 - **verified correct**

2. **Key derivation parameters at `crypto.md:63-64`**:
   - States `m=19,456 KiB, t=2 iterations, p=1 degree`
   - Actual code at `src/crypto/mod.rs:35` uses `Params::new(19_456, 2, 1, Some(32))` - **verified correct**

3. **Legacy format description at `crypto.md:97`**:
   - States "Legacy format (pre-v2) is raw hex without prefix"
   - Actual code at `src/crypto/mod.rs:117-119` checks for v2 prefix and falls back to legacy - **verified correct**

### Potential Issues in Source

1. **Error message for invalid UTF-8** (`src/crypto/mod.rs:99`):
   - Returns `CryptoError::DecryptionFailed("invalid utf-8".to_string())` instead of preserving the more generic "decryption failed" message style
   - Minor inconsistency in error messaging

2. **No migration of legacy data** (`src/crypto/mod.rs`):
   - Comments at `crypto.md:140` state "Legacy ciphertexts are migrated to v2 when `encrypt_provider_keys()` is called"
   - But `encrypt_provider_keys()` is in `config/encryption.rs`, not in this module
   - The crypto module itself has no migration logic - it only decrypts legacy format

---

## Summary of Bugs and Issues

### Bug Reports

| Severity | File:Line | Description |
|----------|------------|-------------|
| Low | `src/exec.rs:176` | Redundant error message: `msg` and `e` both included |
| Low | `src/security/sandbox.rs:237-257` | Static cache never clears, may cause stale path data |
| Info | `src/security/ssrf.rs:27-34` | Confusing dual handling of IPv6 address types |
| Info | `src/permission/mod.rs:1141-1145` | `PermissionResponse` struct defined but unused |
| Info | `src/permission/mod.rs:1237-1248` | `check_external_directory()` marked dead_code, unused |
| Info | `src/crypto/mod.rs:99` | Inconsistent error message format |

### Stale Documentation Summary

| Document | Line(s) | Issue |
|----------|----------|-------|
| `exec.md` | 169 | "Question tool timeout behavior" vague - no timeout actually set |
| `exec.md` | 125-154 | Missing `INTERNAL_ERROR` code used in `print_output()` |
| `security.md` | 120 | `is_available()` description mentions "Linux 5.13+" but no version check exists |
| `security.md` | 197 | `fc00::/7` description says "fd00::/8 only" but actually covers full `fc00::/7` |
| `permission.md` | 61-71 | `PermissionResponse` documented but unused internally |
| `permission.md` | 196-202 | Mode tool lists incomplete - missing `lsp` in review, `write` in docs |
| `crypto.md` | 140 | Claims migration happens in crypto module, but actually in config module |

---

## Improvement Suggestions

### 1. Exec Module

- **Consider adding structured logging**: The `quiet` flag controls `eprintln!` output, but there's no `tracing` instrumentation for exec mode
- **Error message cleanup**: Review `classify_error()` to avoid redundant messages
- **Add `--input-file` and `--output-file` options**: Documented in usage examples but actual CLI flags should be verified

### 2. Security Module

- **Cache invalidation**: The static `CANONICAL_PATHS_CACHE` in `sandbox.rs:237` should either:
  - Be instance-level rather than static, or
  - Include a time-based expiration mechanism
- **Simplify IPv6 handling**: The dual handling of IPv4-mapped addresses could be consolidated
- **Document symlink behavior**: Clarify that `validate_path_safety()` rejects symlinks before checking target - this may surprise users

### 3. Permission Module

- **Remove or use `PermissionResponse`**: The struct at `mod.rs:1141-1145` appears vestigial - either remove it or document its intended purpose
- **Integrate or remove `check_external_directory()`**: This function exists but is never called - either integrate it or remove dead code
- **Document PermissionRegistry session limitation**: The architectural limitation at `permission.md:342-354` is correctly documented but could be addressed by including session context in `perm_id`
- **Consider adding `get_pending_permissions_for_session()`**: Would need architectural changes to PermissionRegistry to store session_id in keys

### 4. Crypto Module

- **Clarify migration responsibility**: Update `crypto.md` to note that legacy migration happens in `config/encryption.rs`, not in the crypto module
- **Add key rotation support**: Currently no mechanism to re-encrypt with new password
- **Consider adding `CryptoError::WrongPassword`**: Distinguish wrong password from other decryption failures

---

## Conclusion

The architecture documents are largely accurate with only minor discrepancies. The most significant issues are:

1. **Documentation gaps** in exec error codes and permission mode tool lists
2. **Dead code** (`PermissionResponse`, `check_external_directory()`) that should be cleaned up
3. **Static cache** in sandbox module that never invalidates

All suggested improvements are low-priority and do not affect functionality.