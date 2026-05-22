# Security Module

The `security` module provides security features for URL validation, IP checking, and sandboxing.

## Overview

**Location**: `src/security/`

**Key Responsibilities**:
- SSRF protection (Server-Side Request Forgery)
- Internal IP validation (IPv4 and IPv6 including IPv4-mapped)
- Symlink detection for path safety
- Landlock sandboxing (Linux)

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
    // - fc00::/7 (IPv6 unique local)
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

Used by: `webfetch`, `websearch`, `codesearch`, `mcp/remote`

### sandbox.rs - Landlock Sandboxing

Linux-specific filesystem sandboxing using Landlock LSM:

```rust
pub struct SandboxConfig {
    pub enabled: bool,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn new() -> Self
    pub fn with_enabled(mut self, enabled: bool) -> Self
    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self
    pub fn with_deny_paths(mut self, paths: Vec<String>) -> Self
    pub fn is_available() -> bool  // Linux 5.13+ with landlock support
    pub fn enforce(&self) -> Result<(), ToolError>
}
```

**Access Flags**:
- `READ` - Read files
- `WRITE` - Write files
- `EXEC` - Execute files

Helper functions:
```rust
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError>
pub fn get_default_allowed_paths() -> Vec<String>
pub fn get_sensitive_paths() -> Vec<String>
```

Used by: `bash` tool for Landlock sandbox enforcement

## Security Flow

### WebFetch Security

```
WebFetch tool
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
| `fc00::/7` | IPv6 unique local |
| `ff00::/8` | IPv6 multicast |
| `::ffff:x.x.x.x` | IPv4-mapped IPv6 |

## Configuration

```toml
[security]
ssrf_protection = true
```

## See Also

- [tool.md](tool.md) - Uses security validation
- [permission.md](permission.md) - Path permissions
- [sandbox skill](../../.opencode/skills/sandbox/SKILL.md) - Sandboxing details