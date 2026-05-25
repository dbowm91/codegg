# Security Module Architecture Review

**Review Date**: 2026-05-25
**Reviewer**: Code review

## Overview

The `security` module provides SSRF protection, IP validation, symlink detection, and Landlock sandboxing. Most implementation details are accurate, but the architecture document has documentation gaps.

---

## Verified Correct Items

### Core SSRF Functions

| Function | Location | Status |
|----------|----------|--------|
| `is_internal_ip(&IpAddr) -> bool` | ssrf.rs:4 | Correct signature and behavior |
| `ipv6_segments_to_ipv4(&Ipv6Addr) -> Option<Ipv4Addr>` | ssrf.rs:39 | Correct - handles both mapped (0xffff) and compatible (0x0) |
| `validate_host_ip(&str, u16) -> Result<Vec<IpAddr>, String>` | ssrf.rs:67 | Correct - resolves DNS, checks internal ranges, checks host string |
| `revalidate_dns(&str, u16, &[IpAddr]) -> Result<(), String>` | ssrf.rs:96 | Correct - DNS rebinding protection with IPv4-mapped equivalence |
| `validate_url_host(&str) -> Result<String, String>` | ssrf.rs:123 | Correct - returns host normalized to lowercase |

### Landlock Sandboxing

| Item | Location | Status |
|------|----------|--------|
| `SandboxConfig` struct | sandbox.rs:6 | Correct - `enabled`, `allowed_paths`, `deny_paths` fields |
| Builder methods | sandbox.rs:14-31 | Correct - `new()`, `with_enabled()`, `with_allowed_paths()`, `with_deny_paths()` |
| `is_available() -> bool` | sandbox.rs:33 | Correct - checks Linux 5.13+ landlock support |
| `enforce() -> Result<(), ToolError>` | sandbox.rs:44 | Correct - enforces landlock on Linux when enabled |
| Helper functions | sandbox.rs:284-321 | Correct - `get_default_allowed_paths()`, `get_sensitive_paths()` |

### Path Safety Validation

| Item | Location | Status |
|----------|----------|--------|
| `validate_path_safety(&Path, &[String]) -> Result<(), ToolError>` | sandbox.rs:259 | Correct - checks symlink BEFORE canonicalization |

### IP Ranges Covered (IPv4)

| Range | Description | Status |
|-------|-------------|--------|
| 127.0.0.0/8 | Loopback | ✓ Correct |
| 10.0.0.0/8 | Private | ✓ Correct |
| 172.16.0.0/12 | Private | ✓ Correct |
| 192.168.0.0/16 | Private | ✓ Correct |
| 169.254.0.0/16 | Link-local | ✓ Correct |
| 0.0.0.0/8 | Current network | ✓ Correct |
| 100.64.0.0/10 | CGNAT | ✓ Correct |
| 198.18.0.0/15 | Benchmark | ✓ Correct |
| 224.0.0.0/4 | Multicast | ✓ Correct |

### IP Ranges Covered (IPv6)

| Range | Description | Status |
|-------|-------------|--------|
| ::1 | Loopback | ✓ Correct |
| fe80::/10 | Unicast link-local | ✓ Correct (via `ipv6.is_unicast_link_local()`) |
| fc00::/8 and fd00::/8 | Unique local | ✓ Correct (`fc00::/7` mask matches both) |
| ff00::/8 | Multicast | ✓ Correct |
| ::ffff:x.x.x.x | IPv4-mapped | ✓ Correct |

---

## Incorrect/Stale Items

### 1. Missing IPv6 Link-Local Documentation

**Location**: architecture/security.md:32

**Issue**: Document lists `fc00::/7 (IPv6 unique local)` but does NOT list IPv6 link-local (`fe80::/10`).

**Current text**:
```
// - ::1 (IPv6 loopback)
// - fc00::/7 (IPv6 unique local)
// - ff00::/8 (IPv6 multicast)
```

**Missing**:
```
// - fe80::/10 (IPv6 link-local)
```

**Fix**: Add `fe80::/10 (IPv6 link-local)` to the list at line 32.

---

### 2. Internal IP Ranges Table Missing Link-Local

**Location**: architecture/security.md:195-197

**Issue**: Table shows IPv6 ranges but omits `fe80::/10` (link-local).

**Current**:
```
| `fc00::/7` | IPv6 unique local |
| `ff00::/8` | IPv6 multicast |
```

**Missing**: `fe80::/10 | IPv6 unicast link-local`

---

### 3. Configuration Section Undocumented

**Location**: architecture/security.md:200-206

**Issue**: Shows minimal config:
```toml
[security]
ssrf_protection = true
```

But `ssrf_protection` is NOT actually used anywhere in the security module. This appears to be a placeholder/example that was never implemented.

**Fix**: Remove the `[security]` configuration example, as no such configuration is loaded by the security module. The landlock configuration must be set programmatically as noted in line 129.

---

## Bugs Found in Related Code

### None

No bugs were found in the security module implementation. The code correctly:
- Checks symlinks before canonicalization (sandbox.rs:260)
- Handles IPv4-mapped IPv6 equivalence in DNS revalidation (ssrf.rs:106-111)
- Uses proper masking for IPv6 unique local (`fc00::/7`)

---

## Summary

| Category | Count |
|----------|-------|
| Items verified correct | 19 |
| Documentation gaps | 2 |
| Configuration issues | 1 |
| Bugs in code | 0 |

---

## Recommended Fixes

1. **Line 32**: Add `fe80::/10 (IPv6 link-local)` to function comment
2. **Lines 195-197**: Add `fe80::/10 | IPv6 unicast link-local` row to Internal IP Ranges table
3. **Lines 200-206**: Remove `[security]` configuration section as it is not implemented
