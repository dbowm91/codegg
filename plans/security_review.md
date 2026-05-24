# Security Module Architecture Review

**Date**: 2026-05-24
**Reviewer**: Architecture Review Agent
**Files Reviewed**:
- `architecture/security.md`
- `src/security/mod.rs`
- `src/security/ssrf.rs`
- `src/security/sandbox.rs`
- `.opencode/skills/security/SKILL.md`
- `.opencode/skills/sandbox/SKILL.md`

---

## Summary

The security module was reviewed against its architecture documentation and skill guides. Overall, the implementation is **correct and well-documented**. Most claims in the architecture document match the actual implementation.

---

## Verified Items

### ssrf.rs - SSRF Protection (Correct)

| Function | Status | Notes |
|----------|--------|-------|
| `is_internal_ip()` | Verified | Correctly blocks all documented ranges including IPv4-mapped IPv6 |
| `ipv6_segments_to_ipv4()` | Verified | Handles both `::ffff:x.x.x.x` and pure IPv6 with segments[5]==0 |
| `validate_host_ip()` | Verified | DNS resolution + IP blocking + host string validation |
| `revalidate_dns()` | Verified | DNS rebinding protection with IPv4-mapped IPv6 equivalence check |
| `validate_url_host()` | Verified | Returns host normalized to lowercase (confirmed at line 144) |

**Internal IP Ranges Blocked (verified)**:
- 127.0.0.0/8 (loopback)
- 10.0.0.0/8 (private)
- 172.16.0.0/12 (private)
- 192.168.0.0/16 (private)
- 169.254.0.0/16 (link-local)
- 0.0.0.0/8 (current network)
- 100.64.0.0/10 (carrier-grade NAT)
- 198.18.0.0/15 (benchmark)
- 224.0.0.0/4 (multicast)
- ::1 (IPv6 loopback)
- fc00::/7 (IPv6 unique local)
- ff00::/8 (IPv6 multicast)
- ::ffff:x.x.x.x (IPv4-mapped IPv6)

### sandbox.rs - Landlock Sandboxing (Correct)

| Function/Struct | Status | Notes |
|-----------------|--------|-------|
| `SandboxConfig` struct | Verified | Has `enabled`, `allowed_paths`, `deny_paths` fields |
| `SandboxConfig::new()` | Verified | Returns default config |
| `with_enabled()` | Verified | Builder method |
| `with_allowed_paths()` | Verified | Builder method |
| `with_deny_paths()` | Verified | Builder method |
| `is_available()` | Verified | Linux only, checks /sys/kernel/security/landlock or /proc/filesystems |
| `enforce()` | Verified | Returns `Ok(())` if disabled, graceful fallback if Landlock unavailable |
| `validate_path_safety()` | Verified | Checks symlink BEFORE canonicalization (line 260) |
| `get_default_allowed_paths()` | Verified | Returns cwd, ~/.config, ~/.local/share, dirs::config_dir, dirs::data_dir |
| `get_sensitive_paths()` | Verified | Returns /etc, /home, /root, /var, /ssh, /proc, /sys, /dev |

---

## Discrepancies Found

### None Critical

The architecture document is **accurate** regarding the security module implementation. No major discrepancies were found.

### Minor Documentation Notes

1. **validate_url_host() return type**: The doc at line 69 shows `Result<String, String>` which is correct. The function returns `host.to_lowercase()` on success.

2. **SandboxConfig not a struct name mismatch**: The architecture doc at line 107-121 correctly shows `SandboxConfig` struct with builder methods. The skill note at line 879 of security/SKILL.md correctly clarifies this.

3. **Access flags documented correctly**: `READ`, `WRITE`, `EXEC` are correctly documented at lines 125-127 in architecture.md.

---

## Bug Status from Previous Reviews

The AGENTS.md notes the following fixes were applied (verified correct):

| Issue | Status |
|-------|--------|
| `validate_url_host()` returns lowercase | **Verified Fixed** - `src/security/ssrf.rs:144` returns `host.to_lowercase()` |
| `validate_path_safety()` symlink check | **Verified Fixed** - `src/security/sandbox.rs:260` checks `symlink_metadata()` before `canonicalize()` |
| `validate_path_safety()` tests added | **Verified Present** - Tests at lines 352-373 cover symlink rejection |

---

## Code Quality Assessment

### Strengths

1. **IPv4-mapped IPv6 handling**: Both `is_internal_ip()` and `revalidate_dns()` correctly handle IPv4-mapped IPv6 addresses by converting them and checking the underlying IPv4 address.

2. **Symlink detection**: Path safety validation correctly checks if the path itself is a symlink BEFORE canonicalization, preventing symlink traversal attacks.

3. **DNS rebinding protection**: `revalidate_dns()` properly detects when IP addresses change between validation and request time.

4. **Graceful Landlock fallback**: `SandboxConfig::enforce()` properly handles systems without Landlock support.

5. **Comprehensive tests**: ssrf.rs has 17 tests covering loopback, private ranges, link-local, multicast, CGNAT, IPv4-mapped, site-local, unicast link-local, and public addresses.

### Potential Issues (Lower Priority)

1. **Error message inconsistency**: `validate_host_ip()` returns `String` errors via `Err(format!(...))` while `validate_path_safety()` returns `ToolError`. This is intentional (different error types for different contexts) but not explicitly documented.

2. **IPv6 unique local check**: In `is_internal_ip()` at line 25, the check `(segments[0] & 0xfe00) == 0xfc00` correctly handles fc00::/7 and fd00::/7 ranges.

---

## Recommendations

### For Documentation

1. **architecture/security.md is accurate** - No major changes needed.

2. **Consider adding**: The "Configuration" section (lines 198-203) shows a minimal config. Could note that Landlock sandboxing configuration is typically in `[security]` section but the actual config loading is handled by the config module.

### For Code

1. **Consider adding tests for `validate_path_safety()` negative cases**: There is a test for symlink rejection but no test for paths that are outside allowed paths but still resolve correctly.

2. **Consider exporting `CANONICAL_PATHS_CACHE` statistics**: For observability, the cache in `sandbox.rs` could log hit/miss rates if debugging is needed.

---

## Conclusion

The security module implementation matches its architecture documentation. The SSRF protection and Landlock sandboxing are correctly implemented with proper handling of IPv4-mapped IPv6 addresses, symlink detection before canonicalization, and DNS rebinding protection.

**No bugs or inconsistencies requiring immediate fixes were found.**

---

## File References

| File | Line(s) | Notes |
|------|---------|-------|
| `src/security/ssrf.rs` | 4-37 | `is_internal_ip()` - all IP ranges blocked |
| `src/security/ssrf.rs` | 39-65 | `ipv6_segments_to_ipv4()` - IPv4-mapped handling |
| `src/security/ssrf.rs` | 67-94 | `validate_host_ip()` - DNS resolution and blocking |
| `src/security/ssrf.rs` | 96-121 | `revalidate_dns()` - DNS rebinding protection |
| `src/security/ssrf.rs` | 123-145 | `validate_url_host()` - URL parsing and validation |
| `src/security/sandbox.rs` | 6-11 | `SandboxConfig` struct definition |
| `src/security/sandbox.rs` | 259-282 | `validate_path_safety()` - symlink check before canonicalize |
| `src/security/sandbox.rs` | 284-308 | `get_default_allowed_paths()` |
| `src/security/sandbox.rs` | 310-321 | `get_sensitive_paths()` |
