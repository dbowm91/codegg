# Security Module

The `security` module provides security features for URL validation, IP checking, and sandboxing.

## Overview

**Location**: `src/security/`

**Key Responsibilities**:
- SSRF protection (Server-Side Request Forgery)
- Internal IP validation
- Path safety validation
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
    // - 0.0.0.0/32 (unspecified)
    // - 100.64.0.0/10 (CGNAT)
    // - 198.18.0.0/15 (benchmark)
    // - 224.0.0.0/4 (multicast)
    // - ::1 (IPv6 loopback)
    // - fe80::/10 (unicast link-local)
    // - fc00::/7 (IPv6 unique local)
    // - :: (IPv6 unspecified)
    // - IPv4-mapped IPv6 addresses
}
```

Note: Takes `&IpAddr`, not `&str` as shown in older documentation.

### validate_url_host()

```rust
pub fn validate_url_host(url: &str) -> Result<String, String> {
    // 1. Parse URL
    // 2. Check scheme (http/https only)
    // 3. Resolve DNS
    // 4. Check IP against internal ranges
}
```

Note: Takes `&str`, not `&Url` as shown in older documentation.

### validate_host_ip()

```rust
pub fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    // 1. Resolve host to IP addresses
    // 2. Check each IP against internal ranges
    // 3. Return validated IPs for later revalidation
}
```

### revalidate_dns()

```rust
pub fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), String> {
    // 1. Re-resolve host
    // 2. Compare with previously validated IPs
    // 3. Detect DNS rebinding attacks
    // 4. Handles IPv4-mapped IPv6 addresses
}
```

### validate_path_safety()

```rust
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError> {
    // 1. Canonicalize path
    // 2. Check against allowed paths
    // 3. Return error if outside allowed paths
}
```

Path safety for file operations.

## Components

### ssrf.rs - SSRF Protection

Prevents requests to internal infrastructure. Contains standalone functions (no `SSRFChecker` struct):

| Function | Description |
|----------|-------------|
| `is_internal_ip()` | Check if IP is internal |
| `ipv6_segments_to_ipv4()` | Extract IPv4 from IPv4-mapped IPv6 |
| `validate_host_ip()` | Resolve host and validate IPs |
| `validate_url_host()` | Parse URL and validate host |
| `revalidate_dns()` | Detect DNS rebinding attacks |

### sandbox.rs - Landlock Sandboxing

Linux-specific filesystem sandboxing using Landlock LSM:

```rust
#[derive(Clone, Debug, Default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn new() -> Self;
    pub fn with_enabled(mut self, enabled: bool) -> Self;
    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self;
    pub fn with_deny_paths(mut self, paths: Vec<String>) -> Self;
    pub fn is_available() -> bool;  // Check if Landlock is supported
    pub fn enforce(&self) -> Result<(), ToolError>;  // Apply restrictions
}
```

**Access Flags** (via Landlock syscalls):
- `READ` - Read files (1<<0)
- `WRITE` - Write files (1<<1)
- `EXEC` - Execute files (1<<2)

**Key helper functions**:
```rust
pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError>;
pub fn get_default_allowed_paths() -> Vec<String>;
pub fn get_sensitive_paths() -> Vec<String>;
```

## Security Flow

### WebFetch Security

```
WebFetch tool
    │
    ▼
validate_url_host(url)
    │
    ├── Parse URL (check http/https)
    ├── DNS resolution
    ├── IP check (is_internal_ip?)
    │     │
    │     ├── Internal → Error
    │     └── External → Continue
    └── Return host

Then before HTTP request:
revalidate_dns(host, port, validated_ips)
    │
    ├── Re-resolve DNS
    ├── Compare with validated IPs
    ├── IPv4-mapped IPv6 handled
    │     │
    │     ├── Same IPv4 → Continue
    │     └── Different → Error (DNS rebinding)
    └── Proceed with request
```

### File Operation Security

```
File tool
    │
    ▼
validate_path_safety(path, allowed_paths)
    │
    ├── Canonicalize (resolve symlinks)
    ├── Check against allowed paths
    │     │
    │     ├── In allowed → Continue
    │     └── Not in allowed → Error
    └── Return (allow operation)
```

### BashTool Landlock Enforcement

```
BashTool::execute()
    │
    ▼
check_command_security()
    │
    ├── Check blocked commands (HashSet)
    ├── Check allowlist if configured
    ├── Check blocked patterns (Regex)
    └── All pass → Continue

Then if landlock_sandbox enabled:
sandbox_config.enforce()
    │
    ├── Check is_available()
    ├── Create Landlock ruleset (syscall 451)
    ├── Add allowed path rules (syscall 452)
    ├── Add deny path rules
    ├── restrict_self (syscall 453)
    └── Enforce sandbox
```

## Error Handling

All security failures use `ToolError::Permission`:
```rust
ToolError::Permission("access to internal addresses not allowed".to_string())
ToolError::Permission("DNS rebinding attack detected".to_string())
ToolError::Permission("path is not in allowed paths".to_string())
```

This maps to HTTP 403 Forbidden in the server.

## Configuration

Security settings are passed programmatically via builder pattern:

```rust
// Via BashTool
BashTool::new()
    .with_landlock_sandbox(true)  // Uses default paths
    .with_landlock_sandbox_custom(config)  // Custom config

// Or via SandboxConfig directly
let config = SandboxConfig::new()
    .with_enabled(true)
    .with_allowed_paths(get_default_allowed_paths())
    .with_deny_paths(get_sensitive_paths());
config.enforce()?;
```

## See Also

- [tool.md](tool.md) - Uses security validation
- [permission.md](permission.md) - Path permissions
- `.opencode/skills/sandbox/SKILL.md` - Sandboxing details
- `.opencode/skills/security/SKILL.md` - Security implementation guide