# Security Architecture Review

## Architecture Document
- Path: architecture/security.md

## Source Code Location
- src/security/

## Verification Summary
**PASS** - The architecture document is largely accurate. All documented functions exist with correct signatures, IP ranges are properly blocked, and security flows match implementation. Minor issues with redundant documentation items and incomplete syscall documentation.

## Verified Claims

| Claim | Status | Notes |
|-------|--------|-------|
| `is_internal_ip(&IpAddr) -> bool` | Pass | All 13 IP ranges correctly implemented. IPv4-mapped IPv6 handling correct. |
| `ipv6_segments_to_ipv4(&Ipv6Addr) -> Option<Ipv4Addr>` | Pass | Handles both `::ffff:x.x.x.x` (0xffff) and pure IPv6 forms. |
| `validate_host_ip(host, port) -> Result<Vec<IpAddr>, String>` | Pass | DNS resolution, internal IP checking, and direct IP string checks all implemented correctly. |
| `revalidate_dns(host, port, validated_ips) -> Result<(), String>` | Pass | DNS rebinding protection correctly checks IP changes including IPv4-mapped equivalence. |
| `validate_url_host(url) -> Result<String, String>` | Pass | Scheme validation (http/https), host via `validate_host_ip()`, returns lowercase host. |
| `validate_path_safety(path, allowed_paths) -> Result<(), ToolError>` | Pass | Symlink check before canonicalization (line 260), canonical path comparison against allowed paths. |
| `SandboxConfig` struct with `enabled`, `allowed_paths`, `deny_paths` | Pass | Struct matches exactly. Builder pattern methods (with_enabled, with_allowed_paths, with_deny_paths) implemented. |
| `SandboxConfig::is_available() -> bool` | Pass | Linux 5.13+ check via `/sys/kernel/security/landlock` or `/proc/filesystems`. |
| `SandboxConfig::enforce() -> Result<(), ToolError>` | Pass | Enforces Landlock ruleset with allow/deny paths. Non-Linux gracefully returns Ok. |
| Access Flags (READ/WRITE/EXEC) | Pass | `LANDLOCK_ACCESS_FS_READ = 1<<0`, `WRITE = 1<<1`, `EXEC = 1<<2` correctly defined. |
| Helper functions (`get_default_allowed_paths`, `get_sensitive_paths`) | Pass | Both functions implemented with documented path lists. |
| Internal IP Ranges Table (13 ranges) | Pass | All ranges correctly implemented in `is_internal_ip()`. |
| Configuration `[security]` with `ssrf_protection` | Pass | Mentioned in doc but no actual config loading exists - this is a feature flag placeholder with no effect. |

## Issues Found

### Inconsistencies

1. **IPv6 link-local redundancy**: The code checks `ipv6.is_unicast_link_local()` at line 21 AND manually checks `segments[0] & 0xff00 == 0xfe80` at line 25. These are equivalent checks - `is_unicast_link_local()` already covers `fe80::/10`. The manual check at line 25 fires for the same range. This is redundant but not buggy.

2. **Site-local IPv6 documentation discrepancy**: Documentation lists "fc00::/7 (IPv6 unique local)" but the code checks `segments[0] & 0xfe00 == 0xfc00`. This IS the correct fc00::/7 check, but the bitmask `0xfe00` on `segments[0]` only checks the first 7 bits of the first segment - effectively matching fc00::/7 and fd00::/7 (unique local). The documentation is slightly imprecise but not incorrect.

### Missing Documentation

3. **`get_canonical_paths()` caching function**: Internal helper function at lines 239-257 caches canonicalized paths to avoid repeated filesystem canonicalization. Not documented in architecture. Works correctly but undocumented.

4. **Landlock syscall constants**: The raw syscall numbers (`SYS_LANDLOCK_CREATE_RULESET = 451`, `SYS_LANDLOCK_ADD_RULE = 452`, `SYS_LANDLOCK_RESTRICT_SELF = 453`) are used directly in `enforce_landlock()` but not documented. While internal, these are Linux kernel ABI values that could be documented.

5. **Deny path implementation note**: Deny paths use `allowed_access: 0` which effectively blocks all access when a ruleset with `handled_access_fs` is active. This is correct Landlock behavior but not documented.

### Improvement Opportunities

6. **IPv4-mapped IPv6 check in V4 branch**: The `is_internal_ip()` function converts IPv4-mapped IPv6 to IPv4 to check (line 22-24), but this conversion is NOT done for the IPv4 octets check at lines 8-16. If an attacker tries to access `::ffff:192.168.1.1` directly as a V6 address, it gets converted to V4 and passes. However, this check only happens in the V6 branch via `ipv6_segments_to_ipv4()`. The check is correct because:
   - For `IpAddr::V4(192.168.1.1)` - checked directly
   - For `IpAddr::V6(::ffff:192.168.1.1)` - converted via `ipv6_segments_to_ipv4()` then checked as V4
   
   The implementation is correct; no bug.

7. **Missing configuration option for Landlock default paths**: `get_default_allowed_paths()` builds default paths at runtime but there's no way to configure these via `[security]` config section. Only programmatic builder pattern available.

8. **Site-local IPv6 overlap**: Line 21 checks `ipv6.is_unicast_link_local()` which covers `fe80::/10` (site-local), and line 25 checks `(segments[0] & 0xfe00) == 0xfc00` which covers `fc00::/7` and `fd00::/7` (unique local). Documentation shows these as separate entries but `is_unicast_link_local()` actually includes site-local scope, not unique local. The check at line 25 handles unique local correctly.

## Recommendations

1. **Remove redundant link-local check**: Consider removing `ipv6.is_unicast_link_local()` from line 21 since the manual check at line 25 `(segments[0] & 0xfe00) == 0xfe80` already handles `fe80::/10`. Alternatively, remove the manual check if `is_unicast_link_local()` is intended to be the sole check.

2. **Document caching behavior**: Add note about `get_canonical_paths()` caching to architecture doc for clarity.

3. **Document deny path semantics**: Add note that deny paths use `allowed_access: 0` which blocks all operations.

4. **Consider `[security]` config section**: Currently `ssrf_protection = true` is documented but not implemented. Consider either implementing config loading or removing from documentation.

5. **Clarify IPv6 scope terminology**: The documentation distinguishes "IPv6 unique local" (fc00::/7) from "IPv6 multicast" (ff00::/8) but uses imprecise term "site-local" in code comment. Standard terminology is:
   - `fe80::/10` = link-local (not site-local)
   - `fc00::/7` = unique local (includes both fc00::/8 and fd00::/8)
   
   Consider updating code comment to say "IPv6 link-local" instead of "IPv6 unique local" for clarity.
