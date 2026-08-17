# Security Module

The `security` module provides security features for URL validation, IP checking, and sandboxing.

## Overview

**Location**: `src/security/` (SSRF, sandbox, sensitive-path policy) and `crates/eggsentry/` (deterministic security scanning — see [native_crates.md](native_crates.md))

**Key Responsibilities**:
- SSRF protection (Server-Side Request Forgery)
- Internal IP validation (IPv4 and IPv6 including IPv4-mapped)
- Symlink detection for path safety
- Landlock sandboxing (Linux)
- Codegg-side re-exports of `eggsentry::{command, dependency, finding, profile, scanner}` for backward-compatible `crate::security::finding::Severity` style paths used by other modules
- Permission policy: `crate::security::policy` (decides Observe/Ask/Deny based on Codegg `SecurityConfig`) — kept in codegg core because it depends on codegg config types

## Key Functions

### is_internal_ip()

```rust
pub fn is_internal_ip(ip: &IpAddr) -> bool {
    // Check for:
    // - 127.0.0.0/8 (loopback)
    // - 10.0.0.0/8 (private)
    // - 172.16.0.0/12 (private)
    // - 192.168.0.0/16 (private)
    // - 169.254.0.0/16 (link-local)
    // - 0.0.0.0/8 (current network)
    // - 100.64.0.0/10 (carrier-grade NAT)
    // - 198.18.0.0/15 (benchmark)
    // - 224.0.0.0/4 (multicast)
    // - ::1 (IPv6 loopback)
    // - fc00::/7 (IPv6 unique local: fc00::/8 and fd00::/8)
    // - fe80::/10 (IPv6 link-local)
    // - ff00::/8 (IPv6 multicast)
    // - IPv4-mapped IPv6 (::ffff:x.x.x.x)
}
```

### ipv6_segments_to_ipv4()

```rust
pub fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr> {
    // Converts IPv4-mapped IPv6 addresses (::ffff:x.x.x.x) to IPv4
    // Also handles pure IPv6 addresses with segments[5] == 0
}
```

### validate_host_ip()

```rust
pub fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    // 1. Resolve DNS
    // 2. Check all resolved IPs against internal ranges
    // 3. Also check if host string itself is an internal IP
}
```

### revalidate_dns()

```rust
pub fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), String> {
    // Re-resolves DNS and checks IP hasn't changed (DNS rebinding protection)
    // Handles IPv4-mapped IPv6 equivalence
}
```

### validate_url_host()

```rust
pub fn validate_url_host(url: &str) -> Result<String, String> {
    // 1. Parse URL
    // 2. Check scheme (http/https only)
    // 3. Validate host via validate_host_ip()
    // 4. Returns host normalized to lowercase
}
```

### validate_path_safety()

```rust
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError> {
    // 1. Check if path itself is a symlink
    // 2. Canonicalize path
    // 3. Check against allowed paths
}
```

## Components

### ssrf.rs - SSRF Protection

Prevents requests to internal infrastructure:

```rust
pub fn is_internal_ip(ip: &IpAddr) -> bool
pub fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr>
pub fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, String>
pub fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), String>
pub fn validate_url_host(url: &str) -> Result<String, String>
```

Used by:
- The `builtin` webfetch path - `validate_url_host`,
  `validate_host_ip`, and `revalidate_dns` inside
  `tool::webfetch::execute_builtin` (`src/tool/webfetch.rs`).
  The default `eggsearch` backend delegates SSRF protection to
  the eggsearch subprocess; these calls are only exercised when
  `backend = "builtin"` or `fallback_to_builtin = true`.
- `codesearch` is an eggsearch-backed `repo_search` compatibility alias;
  its external request validation and provider credentials are owned by
  eggsearch rather than CodeGG.
- `mcp/remote` - `validate_url_host` and `validate_host_ip` at
  `src/mcp/remote.rs` (line numbers drift; search for the call
  sites).

### sandbox.rs - Landlock Sandboxing

Linux enforcement is provided by the maintained `landlock` crate and is
performed only by the private one-shot `codegg-sandbox-helper` process. The
daemon never calls `restrict_self()`. The parent resolves the helper only as
the canonical regular executable sibling of the running CodeGG binary;
inherited environment variables, `PATH`, and the target cwd cannot select it.
The parent serializes a bounded `SandboxLaunchSpec` to an owner-only ephemeral
file in the system temporary directory, outside the target cwd, and starts the
helper with a private status pipe.

The helper adds every rule, requires `FullyEnforced` and `no_new_privs`, and
writes a versioned, length-bounded typed setup frame to that pipe. It marks the
status writer close-on-exec before replacing itself with the target. An exec
failure is reported as a separate terminal frame. The managed parent accepts
only the expected setup/exec state sequence, fails closed on missing,
malformed, duplicate, oversized, or contradictory frames, and never scans or
strips target stdout/stderr for sandbox control text.

The policy is an allow-list. Paths outside the read/write roots are denied by
the handled Landlock rights; `deny_paths` is retained only for source
compatibility and is not represented as a zero-access rule. Raw syscall
numbers and handwritten access masks are not used.

#### Platform and enforcement outcomes

The maintained Landlock backend is supported on Linux hosts whose kernel
exposes the required ABI and filesystem rules. The helper reports the
effective ABI only after all rules are installed, full enforcement is active,
and `no_new_privs` is set. A required sandbox request fails before the target
starts when the helper is missing, the host cannot provide the required ABI,
policy construction fails, or the status channel is incomplete. There is no
silent downgrade from a required request.

On non-Linux hosts, or on Linux hosts without usable Landlock, Python's
portable fallback uses a sanitized environment, workspace-contained cwd, and
snapshot-based post-execution checks. It is not an OS filesystem sandbox and
must be reported as a fallback. Read-only profiles permit workspace/runtime
reads; workspace-write profiles permit writes only under the workspace root.
The daemon itself is never confined by a child-only Landlock policy: only the
one-shot helper applies the policy to its target process.

The typed launch contract distinguishes `Enforced { abi }`, unavailable,
policy/setup failure, and disabled/fallback outcomes. Required requests must
stop before target execution when the helper or kernel cannot enforce the
rules. Best-effort callers may use the explicitly reported portable fallback.

Helper functions:
```rust
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError>
pub fn get_default_allowed_paths() -> Vec<String>
pub fn get_sensitive_paths() -> Vec<String>
```

Used by: the Python executor and the Bash tool through the one-shot helper.
`SandboxConfig::enforce()` intentionally refuses to restrict the calling
process; callers must use the child launch path.

## Security Flow

### WebFetch Security

SSRF protection is applied in two places:

- The `builtin` webfetch path (`tool::webfetch::execute_builtin`),
  which runs when `[search].backend = "builtin"` or as a fallback
  when `fallback_to_builtin = true`:
  ```
  WebFetch tool -> search_backend::dispatch_web_fetch
      │            (builtin branch)
      ▼
  tool::webfetch::execute_builtin
      │
      ▼
  validate_url_host(url)
      │
      ├── Parse URL (scheme check: http/https only)
      ├── validate_host_ip(host, port)
      │     ├── DNS resolution
      │     └── Check IPs against internal ranges
      │
      ▼
  validate_host_ip() returns validated_ips
      │
      ▼
  revalidate_dns() before HTTP request
      │ (detects DNS rebinding attacks)
      ▼
  HTTP request
  ```
- The default `eggsearch` backend delegates SSRF protection to
  the eggsearch subprocess. Codegg does not duplicate the
  full IP-range check on the eggsearch path. However, the
  eggsearch adapter (`search_backend/eggsearch.rs`) now performs
  basic URL validation in `validate_fetch_url()`: rejects empty
  URLs, URLs exceeding 2048 bytes, and non-http/https schemes.
  This validation is applied in `call_web_fetch()` and
  `call_batch_fetch()` before the request reaches eggsearch.

### Path Safety Validation

```
validate_path_safety(path, allowed_paths)
    │
    ├── Check if path is symlink → reject
    │
    ▼
Canonicalize path
    │
    ▼
Check against allowed_paths
    │
    ├── Match → Allow
    └── No match → Reject
```

## Internal IP Ranges Blocked

| Range | Description |
|-------|-------------|
| `127.0.0.0/8` | Loopback |
| `10.0.0.0/8` | Private |
| `172.16.0.0/12` | Private |
| `192.168.0.0/16` | Private |
| `169.254.0.0/16` | Link-local |
| `0.0.0.0/8` | Current network |
| `100.64.0.0/10` | Carrier-grade NAT |
| `198.18.0.0/15` | Benchmark |
| `224.0.0.0/4` | Multicast |
| `::1` | IPv6 loopback |
| `fc00::/7` | IPv6 unique local: fc00::/8 and fd00::/8 |
| `fe80::/10` | IPv6 link-local |
| `ff00::/8` | IPv6 multicast |
| `::ffff:x.x.x.x` | IPv4-mapped IPv6 |

**Note**: `CANONICAL_PATHS_CACHE` is a static cache with a 300-second TTL and 100-entry cap (see `src/security/sandbox.rs:259-286`). Entries older than 300s are evicted on access; the cache is capped at 100 entries.

## See Also

- [tool.md](tool.md) - Uses security validation
- [permission.md](permission.md) - Path permissions
- [security.md](security.md) - Sandboxing details

## Tool Backend (MCP fallback semantics)

`SecurityTool` (`src/tool/security.rs`) wraps `eggsentry` and is
registered by `ToolRegistry::with_options` based on
`[tool_backends.security]` in the loaded `Config`. The matrix mirrors
LSP and is reflected exactly in `ToolRegistry::backend_report(...)`:

| `[tool_backends.security]` setting | Registered tool | `backend_report` status |
|-------------------------------------|-----------------|-------------------------|
| `backend = "native"` (default) or `"builtin"` | real `SecurityTool` wrapper around `eggsentry` | `ready` |
| `backend = "mcp", fallback_to_native = true` (default for `mcp`) | real `SecurityTool` wrapper (the live path is the native crate, not an MCP server) | `fallback-native` |
| `backend = "mcp", fallback_to_native = false` | hidden `DisabledTool` stub — model never sees `security` | `unavailable` (`ConfiguredButUnavailable`) regardless of MCP server connectivity |
| `backend = "disabled"` | hidden `DisabledTool` stub — model never sees `security` | `disabled` |

`DisabledTool` overrides `Tool::expose_in_definitions()` to `false`,
so the stub is registered (callable by name for `/tool-backends`
diagnostics and tests) but filtered from the model-facing tool
definitions. `SecurityTool::execute_structured()` reports provenance
with `backend = "native"`, `implementation = "eggsentry"` when called
through `ToolRegistry::execute_capture` from the agent loop.
# Specialized agent finalization

The `security-review` agent uses the read-only host workflow to prepare a
bounded `SecurityEvidenceBundle`. Its ordinary model turn requests the common
structured report shape, but that request is advisory: the host parses the
public assistant text locally, enforces report bounds, and validates every
finding against prepared targets and evidence. Unsupported findings become
explicit evidence gaps/review prompts; malformed or oversized output fails the
specialized turn and cannot publish successful completion.
