# Security Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `is_internal_ip()` checks 127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 0.0.0.0/8, 100.64.0.0/10, 198.18.0.0/15, 224.0.0.0/4 | VERIFIED | ssrf.rs:6-16 matches exactly |
| `is_internal_ip()` checks IPv6 loopback ::1, fc00::/7, ff00::/8 | VERIFIED | ssrf.rs:20,25-26,27-34 |
| `is_internal_ip()` handles IPv4-mapped IPv6 (::ffff:x.x.x.x) | VERIFIED | ssrf.rs:22-24, ipv6_segments_to_ipv4() at ssrf.rs:39-65 |
| `ipv6_segments_to_ipv4()` converts ::ffff:x.x.x.x to IPv4 | VERIFIED | ssrf.rs:47-54 |
| `ipv6_segments_to_ipv4()` handles pure IPv6 with segments[5] == 0 | VERIFIED | ssrf.rs:55-62 - handles non-mapped IPv4-mapped too |
| `validate_host_ip()` resolves DNS and checks IPs against internal ranges | VERIFIED | ssrf.rs:67-94 |
| `validate_host_ip()` checks if host string itself is an internal IP | VERIFIED | ssrf.rs:84-91 |
| `revalidate_dns()` re-resolves DNS and checks IP hasn't changed | VERIFIED | ssrf.rs:96-121 |
| `revalidate_dns()` handles IPv4-mapped IPv6 equivalence | VERIFIED | ssrf.rs:106-111 |
| `validate_url_host()` parses URL and checks scheme (http/https only) | VERIFIED | ssrf.rs:123-131 |
| `validate_url_host()` validates host via validate_host_ip() | VERIFIED | ssrf.rs:142 |
| `validate_url_host()` returns host normalized to lowercase | VERIFIED | ssrf.rs:144 |
| `validate_path_safety()` checks if path itself is a symlink | VERIFIED | sandbox.rs:236-241 |
| `validate_path_safety()` canonicalizes path | VERIFIED | sandbox.rs:243-245 |
| `validate_path_safety()` checks against allowed paths | VERIFIED | sandbox.rs:247-254 |
| `SandboxConfig::new()` creates default config | VERIFIED | sandbox.rs:12-14 |
| `SandboxConfig::with_enabled()` builder method | VERIFIED | sandbox.rs:16-19 |
| `SandboxConfig::with_allowed_paths()` builder method | VERIFIED | sandbox.rs:21-24 |
| `SandboxConfig::with_deny_paths()` builder method | VERIFIED | sandbox.rs:26-29 |
| `SandboxConfig::is_available()` checks Linux 5.13+ Landlock support | VERIFIED | sandbox.rs:31-40 |
| `SandboxConfig::enforce()` applies sandbox restrictions | VERIFIED | sandbox.rs:42-69 |
| `get_default_allowed_paths()` returns allowed paths | VERIFIED | sandbox.rs:262-286 |
| `get_sensitive_paths()` returns sensitive paths | VERIFIED | sandbox.rs:288-299 |
| Landlock access flags (READ, WRITE, EXEC) | VERIFIED | sandbox.rs:86-88 |
| Internal IP ranges blocked table accurate | VERIFIED | All ranges documented match code |

### Documentation Discrepancies (Non-Critical)

| Item | Status | Note |
|------|--------|------|
| Architecture shows `is_internal_ip(&str)` signature | INCORRECT | Actual is `is_internal_ip(&IpAddr)` - arch doc outdated |
| Architecture shows `validate_url_host(&Url)` signature | INCORRECT | Actual is `validate_url_host(&str)` - arch doc outdated |
| Architecture shows `SSRFChecker` struct | INCORRECT | Struct doesn't exist - standalone functions used |
| IPv6 link-local (fe80::/10) via `is_unicast_link_local()` | UNDOCUMENTED | Code handles it but arch doc doesn't list fe80::/10 |
| IPv4-mapped detection for non-0xffff case | UNDOCUMENTED | segments[5]==0 path handles pure IPv4-mapped too |

## Bugs/Discrepancies Found

### Medium

**1. validate_path_safety() canonicalizes allowed_paths on every call**
- **Location**: sandbox.rs:247-250
- **Issue**: `std::fs::canonicalize(allowed)` is called for every `allowed` path on every invocation of `validate_path_safety()`. If `allowed_paths` contains many paths or `validate_path_safety()` is called frequently, this is wasteful.
- **Impact**: Performance degradation with large allowed_paths or high-frequency calls
- **Fix**: Canonicalize allowed_paths once at initialization time, store canonical paths

**2. Missing test coverage for ssrf.rs**
- **Location**: src/security/ssrf.rs has no tests
- **Issue**: The critical security functions (`is_internal_ip`, `validate_host_ip`, `revalidate_dns`, `validate_url_host`) have zero unit tests
- **Impact**: No verification of SSRF protection correctness, regression risk
- **Fix**: Add comprehensive tests for:
  - All IP ranges (loopback, private, link-local, multicast, IPv4-mapped)
  - URL parsing edge cases ( schemes, hosts, ports)
  - DNS rebinding detection (IP changes, IPv4-mapped equivalence)

### Low

**3. revalidate_dns() doesn't detect new internal IPs joining resolved set**
- **Location**: ssrf.rs:104-117
- **Issue**: Only checks if current IPs are in validated_ips. If DNS resolves a NEW internal IP that wasn't in original set, it would be detected, but the logic could be clearer.
- **Impact**: Actually works correctly - but edge case where validated_ips was empty would cause all current_ips to be flagged
- **Note**: Current code cannot produce empty validated_ips from validate_host_ip() (would error on first internal IP check), so this is not exploitable

## Improvement Suggestions

### Performance

1. **Canonicalize allowed_paths once at initialization** (priority: low)
   - Currently `validate_path_safety()` canonicalizes all allowed_paths on every call
   - Change: `validate_path_safety()` should accept pre-canonicalized paths
   - Impact: Reduces syscalls significantly in high-frequency path validation scenarios

2. **Consider caching DNS resolution results** (priority: low)
   - `validate_host_ip()` and `revalidate_dns()` both call `to_socket_addrs()`
   - For repeated checks to same host, could cache briefly (TTL of a few seconds)
   - Impact: Reduces DNS lookups, but adds complexity and cache invalidation concerns

### Correctness

1. **Add comprehensive SSRF tests** (priority: medium)
   - Test all internal IP ranges are blocked
   - Test IPv4-mapped addresses are properly handled
   - Test DNS rebinding detection works
   - Test URL scheme validation (http/https only)

2. **Document IPv6 link-local handling in architecture** (priority: low)
   - The code correctly blocks fe80::/10 via `ipv6.is_unicast_link_local()` (line 21)
   - But architecture doc only mentions `fc00::/7` and `ff00::/8` for IPv6 ranges
   - Should document that `fe80::/10` (IPv6 link-local) is also blocked

### Maintainability

1. **Add inline documentation for complex IPv4-mapped handling** (priority: low)
   - ssrf.rs:55-62 handles segments[5]==0 case (non-0xffff mapped)
   - This is for cases like ::ffff:0.0.0.0/104 but the logic is subtle
   - Add comment explaining when this path is taken

2. **Consider extracting IP range checks to a helper struct** (priority: low)
   - `is_internal_ip()` has many inline range checks
   - A `IpRange` struct with `contains(&IpAddr) -> bool` could improve readability
   - But this is optional - current inline checks are clear

3. **Add integration test for Landlock sandbox enforcement** (priority: low)
   - Current tests only cover `validate_path_safety()` logic
   - No tests for actual Landlock syscall sequence
   - Would require root/CAP_SYS_ADMIN in CI to test

## Priority Actions (top 5 items to fix)

1. **Add SSRF tests** (Medium priority)
   - No tests currently exist for `is_internal_ip()`, `validate_host_ip()`, `revalidate_dns()`, `validate_url_host()`
   - Create `src/security/ssrf_tests.rs` or add to existing test module
   - Cover all internal IP ranges, IPv4-mapped, URL validation, DNS rebinding

2. **Fix validate_path_safety() performance** (Low priority)
   - Canonicalize allowed_paths once instead of on every call
   - API change: require pre-canonicalized paths or add initialization helper

3. **Update architecture/security.md signatures** (Low priority)
   - Fix `is_internal_ip(&IpAddr)` (was shown as `&str`)
   - Fix `validate_url_host(&str)` (was shown as `&Url`)
   - Add fe80::/10 to IPv6 ranges table

4. **Add comment for IPv4-mapped non-0xffff case** (Low priority)
   - ssrf.rs:55-62 handles a subtle edge case
   - Document when segments[5]==0 path is taken (e.g., ::127.0.0.1)

5. **Add Landlock integration test** (Future)
   - Requires CAP_SYS_ADMIN or root in CI
   - Test actual syscall sequence succeeds
   - This is blocked by CI environment limitations