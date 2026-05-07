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
pub fn is_internal_ip(ip: &str) -> bool {
    // Check for:
    // - 127.0.0.0/8 (loopback)
    // - 10.0.0.0/8 (private)
    // - 172.16.0.0/12 (private)
    // - 192.168.0.0/16 (private)
    // - ::1 (IPv6 loopback)
    // - fc00::/7 (IPv6 unique local)
}
```

### validate_url_host()

```rust
pub fn validate_url_host(url: &Url) -> Result<(), SecurityError> {
    // 1. Parse URL
    // 2. Resolve DNS (ensure no DNS rebinding)
    // 3. Check IP against internal ranges
    // 4. Check for valid scheme (http/https only)
}
```

SSRF protection for webfetch tool.

### validate_path_safety()

```rust
pub fn validate_path_safety(path: &Path) -> Result<(), SecurityError> {
    // 1. Canonicalize path
    // 2. Check for symlink traversal (..)
    // 3. Check against allowed paths
    // 4. Check for dangerous patterns
}
```

Path safety for file operations.

## Components

### ssrf.rs - SSRF Protection

Prevents requests to internal infrastructure:

```rust
pub struct SSRFChecker {
    allowed_domains: Vec<String>,
    blocked_ips: Vec<Cidr>,
}

impl SSRFChecker {
    pub fn check(&self, url: &Url) -> Result<(), SSRFError>;
}
```

### sandbox.rs - Landlock Sandboxing

Linux-specific filesystem sandboxing using Landlock LSM:

```rust
pub struct LandlockSandbox {
    rules: LandlockRules,
}

impl LandlockSandbox {
    pub fn new() -> Self;
    pub fn add_rule(&mut self, path: &Path, access: AccessFlags);
    pub fn restrict_self(&self) -> Result<(), LandlockError>;
}
```

**Access Flags**:
- `READ` - Read files
- `WRITE` - Write files
- `EXEC` - Execute files

## Security Flow

### WebFetch Security

```
WebFetch tool
    │
    ▼
validate_url_host(url)
    │
    ├── Parse URL
    ├── DNS resolution (with caching)
    ├── IP check (is_internal_ip?)
    │     │
    │     ├── Internal → Error
    │     └── External → Continue
    └── Scheme check (http/https only)
```

### File Operation Security

```
File tool
    │
    ▼
validate_path_safety(path)
    │
    ├── Canonicalize (resolve symlinks)
    ├── Check traversal (..)
    ├── Check against rules
    │     │
    │     ├── Deny → Error
    │     └── Allow → Continue
    └── Return canonical path
```

## Configuration

```toml
[security]
ssrf_protection = true

[security.allowed_domains]
- "api.github.com"
- "api.openai.com"

[security.allowed_paths]
- "/home/user/project"
```

## See Also

- [tool.md](tool.md) - Uses security validation
- [permission.md](permission.md) - Path permissions
- [sandbox skill](../../.opencode/skills/sandbox/SKILL.md) - Sandboxing details
