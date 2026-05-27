# Provider & Resilience & Security Architecture Review

## Verified Claims

### Provider Module (src/provider/)

| Claim | Status | Location |
|-------|--------|----------|
| Provider trait at lines 60-73 | VERIFIED | `src/provider/mod.rs:60-73` |
| ChatRequest struct at lines 97-109 | VERIFIED | `src/provider/mod.rs:97-109` |
| Message enum at lines 111-128 | VERIFIED | `src/provider/mod.rs:111-128` |
| ContentPart enum at lines 130-135 | VERIFIED | `src/provider/mod.rs:130-135` |
| ChatEvent enum at lines 142-156 | VERIFIED | `src/provider/mod.rs:142-156` |
| TokenUsage struct at lines 175-182 | VERIFIED | `src/provider/mod.rs:175-182` |
| ToolDefinition struct at lines 184-210 | VERIFIED | `src/provider/mod.rs:184-210` |
| ProviderRegistry at lines 234-263 | VERIFIED | `src/provider/mod.rs:234-263` |
| register_builtin() function | VERIFIED | `src/provider/mod.rs:265-312` |
| register_builtin_with_config() function | VERIFIED | `src/provider/mod.rs:376-523` |
| EventStream type definition | VERIFIED | `src/provider/mod.rs:58` |
| create_http_client() function | VERIFIED | `src/provider/mod.rs:46-56` |
| ToolDefinition::to_openai() | VERIFIED | `src/provider/mod.rs:192-201` |
| ToolDefinition::to_anthropic() | VERIFIED | `src/provider/mod.rs:203-209` |

### Provider Auto-Registration

| Claim | Status | Evidence |
|-------|--------|----------|
| Only codegg_go auto-registered via register_builtin() | VERIFIED | `src/provider/mod.rs:520-522` - Falls back to register_builtin() only if registry is empty after config-based registration |
| SAP AI Core, Zenmux, Kilo, Vercel AI Gateway are config-only | VERIFIED | `src/provider/additional.rs:151-170` - These factories exist but are not called from register_builtin() |

### Resilience Module (src/resilience/)

| Claim | Status | Location |
|-------|--------|----------|
| CircuitBreaker struct with inner Arc | VERIFIED | `src/resilience/circuit.rs:43-46` |
| CircuitState enum (Closed, Open, HalfOpen) | VERIFIED | `src/resilience/circuit.rs:8-13` |
| CircuitBreakerInner fields using TokioRwLock | VERIFIED | `src/resilience/circuit.rs:30-41` |
| CircuitBreaker::new() with max_half_open_duration=30s | VERIFIED | `src/resilience/circuit.rs:66` |
| is_available() uses write lock from start | VERIFIED | `src/resilience/circuit.rs:79-99` |
| call() checks HalfOpen timeout before executing | VERIFIED | `src/resilience/circuit.rs:114-127` |
| record_success() and record_failure() behavior | VERIFIED | `src/resilience/circuit.rs:139-186` |

### Security Module (src/security/)

| Claim | Status | Location |
|-------|--------|----------|
| is_internal_ip() function | VERIFIED | `src/security/ssrf.rs:4-37` |
| ipv6_segments_to_ipv4() function | VERIFIED | `src/security/ssrf.rs:39-65` |
| validate_host_ip() function | VERIFIED | `src/security/ssrf.rs:67-94` |
| revalidate_dns() function | VERIFIED | `src/security/ssrf.rs:96-121` |
| validate_url_host() function | VERIFIED | `src/security/ssrf.rs:123-145` |
| validate_path_safety() function | VERIFIED | `src/security/sandbox.rs:275-298` |
| SandboxConfig struct | VERIFIED | `src/security/sandbox.rs:24-30` |
| SandboxConfig::is_available() | VERIFIED | `src/security/sandbox.rs:57-66` |
| Landlock syscall constants | VERIFIED | `src/security/sandbox.rs:112-118` |

### FallbackProvider (src/provider/fallback.rs)

| Claim | Status | Evidence |
|-------|--------|----------|
| Default retryable status codes [429, 500, 502, 503, 504] | VERIFIED | Line 17-20 |
| Circuit breaker per provider (failure_threshold=3, timeout_secs=60, success_threshold=2) | VERIFIED | Line 21-24 |
| Exponential backoff formula 2^i seconds | VERIFIED | Line 107: `let delay_secs = (2u64.pow(i as u32)).min(30);` |

## Incorrect/Stale Claims

### 1. SseParser Line Numbers (provider.md:526)

**Issue**: Documentation states `src/provider/sse_parser.rs:16-382` but actual file is 988 lines.

**Current**: The SseParser struct is at lines 16-24, but the documented function range 16-382 is incorrect.

**Recommendation**: Update documentation to reference accurate line range (SseParser struct is lines 16-24).

### 2. IPv6 Unique Local - fc00::/7 Documentation (security.md:197)

**Issue**: Documentation states `fc00::/8 and fd00::/8` for unique local, but code at `src/security/ssrf.rs:25` uses `(segments[0] & 0xfe00) == 0xfc00` which correctly matches fc00::/7 (not just fc00::/8 and fd00::/8).

**Code**: `src/security/ssrf.rs:25` - `(segments[0] & 0xfe00) == 0xfc00` checks the fc00::/7 range properly.

**Recommendation**: Update description to say `fc00::/7 (unique local: fc00::/8 and fd00::/8)`.

### 3. Missing Known Issue in Security Documentation

**Issue**: AGENTS.md lists `STATIC CANONICAL_PATHS_CACHE never clears` as a known issue at `src/security/sandbox.rs:237`, but security.md does not mention it.

**Recommendation**: Add to security.md "Known Issues" section.

## Bugs Found

**None identified** - All verified implementations match their documentation.

## Improvements Identified

1. **Fix SSE Parser line numbers** - Update provider.md:526 to reflect actual SseParser struct location (lines 16-24) or remove specific line references.

2. **Fix IPv6 unique local description** - Change `fc00::/8` to `fc00::/7` in security.md:197.

3. **Sync known issues** - Add CANONICAL_PATHS_CACHE never clears to security.md.

4. **Verify SSE Parser state preservation markers** - Documentation mentions special markers but deeper inspection needed to verify exact format.

## Stale References

- **Line number references** in provider.md become stale as code changes (e.g., "circuit.rs:114-127")
- **"Used by" references** in security.md should be periodically verified against actual tool implementations

## Recommendations

1. Remove specific line numbers from SSE Parser documentation or update to accurate values
2. Fix IPv6 unique local documentation to accurately reflect fc00::/7 range
3. Add Known Issue for CANONICAL_PATHS_CACHE to security.md
4. Consider adding test coverage verification for security modules

## Summary

Overall, the architecture documentation for Provider, Resilience, and Security modules is **highly accurate**. The major issues found are:

- **Line number staleness**: SSE Parser documentation references a line range that no longer matches the file (988 vs 382 lines)
- **Minor documentation clarity**: IPv6 unique local description is slightly misleading
- **Missing known issue sync**: CANONICAL_PATHS_CACHE issue not in security.md

**No actual bugs found in the code** - all verified implementations match their documentation.
