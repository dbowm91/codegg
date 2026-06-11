---
name: security
description: Guide for security implementations in opencode-rs
version: 1.1.0
tags:
  - security
  - permissions
  - path-validation
  - rate-limiting
  - doom-loop
---

# Security Implementation Guide

This skill covers security-related implementations in opencode-rs.

## Path Validation

All file tools must validate that accessed paths stay within allowed boundaries:

```rust
use std::path::{Path, PathBuf};
use crate::error::ToolError;

pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    // CRITICAL: Check for symlinks BEFORE canonicalization
    // symlink_metadata on a canonicalized path will never detect symlinks
    check_path_for_symlinks(path)?;

    let canonical = path.canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))?;

    let root_canonical = allowed_root.canonicalize()
        .map_err(|_| ToolError::Execution("invalid allowed root".to_string()))?;
    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }
    Ok(canonical)
}

pub fn check_path_for_symlinks(path: &Path) -> Result<(), ToolError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if current.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(ToolError::Permission(format!(
                "symlink not allowed in path: {}",
                current.display()
            )));
        }
    }
    Ok(())
}
```

For unrestricted mode (trusted environment), use `canonicalize_path()`:
```rust
pub fn canonicalize_path(path: &Path) -> Result<PathBuf, ToolError> {
    let canonical = path.canonicalize()
        .map_err(|_| ToolError::Execution(format!("invalid path: {}", path.display())))?;

    // Also reject symlinks in unrestricted mode
    if canonical.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(ToolError::Permission("symlinks not allowed".to_string()));
    }
    Ok(canonical)
}
```

Tools with path validation: `read`, `write`, `edit`, `replace`, `multiedit`, `grep`, `glob`, `list`

## TOCTOU Prevention

When validating paths and then operating on them, perform both atomically:

```rust
// ❌ BAD - TOCTOU race
let validated_path = validate_path(path, &self.allowed_root)?;
tokio::fs::write(path, content).await?;

// ✅ GOOD - Atomic validation + operation
let validated_path = validate_path(path, &self.allowed_root)?;
tokio::task::spawn_blocking(move || {
    std::fs::write(&validated_path, content)?;
    Ok(())
}).await??;
```

## Symlink Handling

When walking directories, skip symlinks to prevent traversal attacks:

```rust
let walk = WalkBuilder::new(&search_path)
    .hidden(false)
    .git_ignore(true)
    .build();

for entry in walk {
    let entry = entry.map_err(|e| ToolError::Execution(e.to_string()))?;

    // Skip symlinks
    if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
        continue;
    }
    // ...
}
```

## Write Tool TOCTOU Prevention

The write tool validates parent paths BEFORE creating directories:

```rust
// ✅ GOOD - Validate parent BEFORE create_dir_all
fn write_tool(path: &Path, content: &str) -> Result<(), ToolError> {
    // Get parent and validate it FIRST
    let parent = path.parent().ok_or_else(||
        ToolError::InvalidInput("no parent directory".into())
    )?;

    // For non-existent parents, check symlink components before creation
    if !parent.exists() {
        check_path_for_symlinks(parent)?;
    }

    // NOW create directories
    std::fs::create_dir_all(parent)?;

    // Then write file
    std::fs::write(path, content)?;
    Ok(())
}
```

This prevents TOCTOU races where an attacker could create a symlink between validation and file creation.

## BashTool Security

BashTool blocks dangerous patterns using `expect()` to catch invalid regex at startup:

```rust
static BLOCKED_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    let patterns = vec![
        r"\$\(",           // Command substitution
        r"`",              // Backtick substitution
        r"\|/.*sh",        // Pipe to shell (no whitespace required)
        r"\|/.*bash",      // Pipe to bash (no whitespace required)
        r">/dev/",         // Redirect to dev (no whitespace required)
        r"2>/dev/",        // stderr redirect (no whitespace required)
        r"&&",             // AND operator
        r"\|\|",           // OR operator
        r";\s*",           // Sequential commands
        // ... more patterns
    ];
    patterns
        .into_iter()
        .map(|p| Regex::new(p).expect(&format!("invalid blocked pattern: {}", p)))
        .collect()
});
```

**Important**: Use `.map().expect()` not `.filter_map().ok()` - invalid patterns should fail fast at startup rather than silently being dropped.

Workdir traversal check uses canonicalization:

```rust
let canonical_dir = std::fs::canonicalize(dir)?;
let canonical_path = std::fs::canonicalize(path)?;
if canonical_dir.starts_with(&canonical_path) {
    // allowed
}
```

## DoomLoopDetector

Prevents agents from repeating the same tool call:

```rust
pub struct DoomLoopDetector {
    history: VecDeque<String>,  // Recent tool call names (for ordering)
    counts: HashMap<String, usize>,  // Count of each tool name for O(1) lookup
    max_window: usize,
    threshold: usize,
}

impl DoomLoopDetector {
    pub fn new(max_window: usize, threshold: usize) -> Self;
    pub fn record_tool_call(&mut self, tool_name: &str);
    pub fn is_doom_loop(&self) -> bool;  // O(1) lookup using counts HashMap
    pub fn reset(&mut self);
}
```

**Performance note**: Uses `HashMap` for O(1) `is_doom_loop()` instead of O(n) iteration.

Integration in AgentLoop:

Doom loop detection is integrated with permission checking - it does not bypass permissions:

```rust
self.doom_detector.record_tool_call(&tc.name);
let doom_loop = self.doom_detector.is_doom_loop();

let perm_result = self.permission_checker.check(&tc.name, path.as_deref());
match perm_result {
    PermissionResult::Allow => {
        if doom_loop {
            // Deny only tools that would otherwise be allowed
            tool_results.push((tc.id.clone(), "Tool denied: potential doom loop detected".to_string()));
        } else {
            allowed_tools.push(tc.clone());
        }
    }
    PermissionResult::Deny => {
        // Already denied, doom loop doesn't change this
    }
    PermissionResult::Ask => {
        // Show permission dialog even for doom loop detected tools
        // User can still allow or deny
    }
}
```

## Rate Limiting

Use peer socket address, not headers:

```rust
async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let key = addr.to_string();  // Real peer address
    // ...
}
```

**WebSocket Rate Limiting**: Rate limiting must also be applied to WebSocket endpoints:

```rust
static RATE_LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| RateLimiter::new(100, 60));

async fn upgrade_ws(socket: WebSocket, addr: SocketAddr, ...) {
    // Check rate limit before processing
    if !RATE_LIMITER.check_rate_limit(&addr.to_string()).await {
        // Send 429 response and close
        return;
    }
    // ... continue handling
}
```

## HTTP Header Injection Prevention

When adding custom headers to HTTP requests, validate that header values don't contain newline characters:

```rust
// ❌ BAD - Header values could contain newline characters allowing header injection
for (k, v) in &self.headers {
    request = request.header(k, v);
}

// ✅ GOOD - Validate header values don't contain \r or \n
for (k, v) in &self.headers {
    if v.contains('\r') || v.contains('\n') {
        return Err(McpError::Server("header value contains invalid characters".into()));
    }
    request = request.header(k, v);
}
```

## OAuth redirect_uri Validation

Validate redirect_uri before use in OAuth flows:

```rust
// Validate redirect_uri is HTTPS or localhost for development
let redirect = url::Url::parse(redirect_uri)?;
if redirect.scheme() != "https"
    && redirect.host_str() != Some("localhost")
    && redirect.host_str() != Some("127.0.0.1")
{
    return Err(McpError::OAuth(
        "redirect_uri must use HTTPS or be localhost".into(),
    ));
}
```

## WebFetch SSRF Protection

Validate URLs before fetching:

- Only `http` and `https` schemes allowed
- Block internal hosts: `localhost`, `127.*`, `0.0.0.0`, `::1`, `fc00:*`, `fe80:*`
- IPv4-mapped IPv6 addresses (`::ffff:127.0.0.1`) are blocked
- Redirect following is disabled to prevent redirect-based bypasses
- Use `ToSocketAddrs` to normalize and validate all resolved addresses

**Internal IP ranges blocked:**
- Loopback: `127.*`, `::1`
- Current network: `0.0.0.0/8`
- Link-local: `169.254.0.0/16`, `fe80::/10`
- ULA: `fc00::*`, `fd00::*`
- Carrier-grade NAT: `100.64.0.0/10`
- Benchmark: `198.18.0.0/15`
- IPv4-mapped IPv6: `::ffff:0.0.0.0/104`
- Multicast: `224.*`-`239.*`, `ff00::*`

**IPv4-mapped IPv6 protection** - Must check for IPv4-mapped addresses (`::ffff:x.x.x.x`):

```rust
fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ipv6.segments();
    if segments[0] == 0 && segments[1] == 0 && segments[2] == 0
        && segments[3] == 0 && segments[4] == 0
    {
        if segments[5] == 0xffff {
            return Some(Ipv4Addr::new(
                (segments[6] >> 8) as u8,
                (segments[6] & 0xff) as u8,
                (segments[7] >> 8) as u8,
                (segments[7] & 0xff) as u8,
            ));
        }
    }
    None
}

fn is_internal_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_unicast_link_local()
                || ipv6_segments_to_ipv4(ipv6)
                    .map(|v4| is_internal_ip(&IpAddr::V4(v4)))
                    .unwrap_or(false)
                // ... other checks
        }
        // ...
    }
}
```

This function exists in `src/security/ssrf.rs` and is the canonical location for SSRF protection utilities. Use `crate::security::ssrf::*` to import.

### DNS Rebinding Protection

**Critical**: Always re-validate DNS resolution immediately before making the HTTP request to prevent DNS rebinding attacks:

```rust
// Initial validation - stores validated IPs
fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, ToolError> {
    let socket_addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| ToolError::Execution("cannot resolve host to address".to_string()))?
        .collect();

    let validated_ips: Vec<IpAddr> = socket_addrs.iter().map(|addr| addr.ip()).collect();

    // Check each resolved IP against internal blocklist
    for ip in &validated_ips {
        if is_internal_ip(ip) {
            return Err(ToolError::Execution(
                "access to internal addresses not allowed".to_string(),
            ));
        }
    }

    Ok(validated_ips)
}

// Re-validation before request - detects changed IPs
fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), ToolError> {
    let current_addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| ToolError::Execution("cannot resolve host to address".to_string()))?
        .collect();

    let current_ips: Vec<IpAddr> = current_addrs.iter().map(|addr| addr.ip()).collect();

    // Check if any IP has changed since initial validation
    for ip in &current_ips {
        if !validated_ips.contains(ip) {
            // Check if IPv6 address maps to an IPv4 that was validated
            if let IpAddr::V6(ipv6) = ip {
                if let Some(v4) = ipv6_segments_to_ipv4(ipv6) {
                    if validated_ips.contains(&IpAddr::V4(v4)) {
                        continue;  // IPv4-mapped address, same as already validated
                    }
                }
            }
            return Err(ToolError::Execution(
                "DNS rebinding attack detected: IP address changed".to_string(),
            ));
        }
    }

    Ok(())
}

// Usage: validate once, then re-validate before each request
let validated_ips = validate_host_ip(host, port)?;
revalidate_dns(host, port, &validated_ips)?;  // Before HTTP request
let response = client.get(url).send().await?;
```

This pattern is implemented in `src/tool/webfetch.rs` to prevent attackers from changing DNS between validation and connection.

## Permission System

Path extraction from tool arguments:
- `read`, `write`, `edit`, `replace`, `glob`, `grep`, `list` → `arguments["path"]`
- `apply_patch` → `arguments["patch_path"]`

## Security review workflow

The `src/security/workflow.rs` module provides a structured security review workflow that ties together the existing security infrastructure.

### Workflow types

- `SecurityReviewTarget` — A file/location selected for review, with preset and reason
- `SecurityReviewFinding` — Evidence-based finding with severity, confidence, evidence, reasoning, recommendation
- `SecurityReviewPrompt` — Marker-only triage prompt (not a confirmed finding)
- `SecurityPreflightResult` — Deterministic check result (pass/fail/skipped)
- `SecurityReviewOutput` — Complete workflow output combining all of the above

### Target discovery

`discover_targets_from_diff()` uses `egggit` to parse unified diffs and produce `SecurityReviewTarget` instances. Files are filtered through `should_skip_file()` which excludes binary, vendor, and generated paths.

### Preset selection

`select_preset_for_file()` maps file paths to `securityContext` presets deterministically:
- `Cargo.toml`, `build.rs` → `dependency_review`
- Files with "unsafe" in name → `unsafe_review`
- Auth/middleware/handler/route paths → `web_backend`
- CLI/command/process paths → `rust_cli`
- Default → `rust_server`

### Finding synthesis

`synthesize_findings()` enforces the marker-vs-finding distinction:
- Risk markers without additional evidence → `SecurityReviewPrompt`
- Risk markers with changed code + plausible flow → `SecurityReviewFinding`
- Preflight failures on changed lines → `SecurityReviewFinding`
- Never emit a finding from a marker alone

### Invoking the workflow

- Slash command: `/security-review [--changed] [--file <path>] [--preset <name>] [--deep]`
- Subagent: spawn `security-review` agent via task tool
- Internal: call `discover_targets_from_diff()` + `run_preflight_checks()` + `synthesize_findings()` directly

## Security Checklist

When implementing new tools or modifying existing ones:

1. **Path Validation**: Does the tool access files? Add `allowed_root` and `unrestricted` fields
2. **TOCTOU**: Are validation and operation separate? Combine into atomic `spawn_blocking`
3. **Symlinks**: Does tool walk directories? Skip symlinks, verify canonical paths
4. **Command Injection**: Does tool execute commands? Add security pattern checks
5. **Doom Loop**: Could tool be called repeatedly? Consider DoomLoopDetector integration
6. **Rate Limiting**: Is tool exposed via HTTP? Use peer address for rate limits

When implementing OAuth flows:

7. **State Replay**: Store used authorization codes with expiration, reject replays
8. **Token Security**: Use secure random generation for tokens/secrets
9. **redirect_uri**: Validate redirect_uri is HTTPS or localhost (never allow HTTP redirects)

When adding custom headers to HTTP requests:

10. **Header Injection**: Validate header values don't contain `\r` or `\n`

When implementing WASM plugins:

9. **Fuel Limits**: Set appropriate fuel limits per hook (1M), track global budget
10. **Module Size**: Validate WASM module size before compilation (max 10MB)
11. **Timeout**: Wrap hook execution in timeout (30s recommended)
12. **Symlinks**: Reject symlinks during plugin installation

When exposing data via API:

13. **Secrets Redaction**: Scrub sensitive data (API keys, tool inputs/outputs) before export
14. **Token Expiration**: Use time-limited tokens for share URLs (7 days recommended)

## Input Validation Patterns

### Blocked Command Detection
Use `starts_with()` to check if the command starts with a blocked command pattern:

```rust
// ❌ BAD - checking individual words fails for multi-word blocked commands
let words: HashSet<&str> = normalized.split_whitespace().collect();
if self.blocked_commands.iter().any(|c| words.contains(c)) {
    return Err(ToolError::Permission("command matches blocked list".to_string()));
}

// ✅ GOOD - check if command starts with any blocked pattern
for blocked_cmd in &self.blocked_commands {
    if normalized.starts_with(blocked_cmd) {
        return Err(ToolError::Permission(format!(
            "command matches blocked list: {}",
            blocked_cmd
        )));
    }
}
```

### Regex Complexity Limits (ReDoS Prevention)

Prevent ReDoS attacks by limiting pattern size and capture groups:

```rust
const MAX_PATTERN_SIZE: usize = 4096;
const MAX_PATTERN_GROUPS: usize = 32;

if pattern.len() > MAX_PATTERN_SIZE {
    return Err(ToolError::Execution(format!("pattern exceeds {} bytes", MAX_PATTERN_SIZE)));
}

let group_count = pattern.matches('(').count();
if group_count > MAX_PATTERN_GROUPS {
    return Err(ToolError::Execution(format!("too many capture groups")));
}
```

### External API Query Sanitization

Validate and sanitize before sending to external services:

```rust
const MAX_QUERY_LENGTH: usize = 10000;

if query.len() > MAX_QUERY_LENGTH {
    return Err(ToolError::Execution("query too long".to_string()));
}

let sanitized: String = query
    .chars()
    .filter(|&c| !c.is_control() && c != '\'' && c != '"' && c != ';' && c != '\\' && c != '\0')
    .collect();
```

### Batch Tool Input Validation

Validate tool names and input sizes to prevent abuse:

```rust
const MAX_CALL_INPUT_SIZE: usize = 100_000;
const MAX_BATCH_OUTPUT_SIZE: usize = 500_000;

// Validate tool name format
if !tool_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
    return Err(ToolError::Execution("invalid tool name characters".to_string()));
}

// Truncate output to prevent memory exhaustion
if output.len() > MAX_BATCH_OUTPUT_SIZE {
    output.truncate(MAX_BATCH_OUTPUT_SIZE);
    output.push_str("... [output truncated]");
}
```

### Unrestricted Mode Warnings

Log when tools run without path validation:

```rust
if self.unrestricted {
    tracing::warn!("Tool executing with unrestricted=true - no path validation");
}
```

## Error Handling Patterns

### AppError IntoResponse

`AppError` implements `axum::response::IntoResponse` (feature-gated with `server` feature):

```rust
#[cfg(feature = "server")]
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        // Returns appropriate status code and JSON error body
        // based on error variant
    }
}
```

Use proper `AppError` types in server routes instead of returning `StatusCode` directly:

```rust
// ❌ BAD - Bypasses error handling system
pub async fn handler(...) -> Result<Json<Response>, StatusCode> {
    Err(StatusCode::BAD_REQUEST)
}

// ✅ GOOD - Uses AppError with proper HTTP mapping
pub async fn handler(...) -> Result<Json<Response>, AppError> {
    Err(AppError::Storage(StorageError::NotFound(msg)))
}
```

### Error Variants

Key error types in `src/error.rs`:

- `ProviderError::Timeout(String)` - Provider timeout
- `McpError::Timeout(String)` - MCP timeout
- `ToolError::Disabled(String)` - Tool disabled
- `PluginError::LoadFailed(#[from] LoadError)` - Plugin load failed
- `PluginError::InstallFailed(#[from] InstallError)` - Plugin install failed

### SessionSummaryProvider Error Type

The `SessionSummaryProvider` trait uses `AppError` (not `anyhow::Error`):

```rust
#[async_trait::async_trait]
pub trait SessionSummaryProvider: Send + Sync {
    async fn generate_summary(&self, conversation: &str) -> Result<String, AppError>;
    async fn generate_title(&self, conversation: &str) -> Result<String, AppError>;
}
```

## WASM Plugin Security

### Fuel Limits

WASM plugins have fuel (instruction budget) limits to prevent DoS:

```rust
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;  // 10MB
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;       // 1M fuel
const WASM_HOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
```

Fuel is tracked per-plugin in `ModuleCache::fuel_budgets`. Before executing a hook, fuel is reserved and checked:

```rust
let current_plugin_fuel = fuel_budgets.get(plugin_id).map(|v| v.load(Ordering::Relaxed)).unwrap_or(MAX_PLUGIN_FUEL_BUDGET);
if current_plugin_fuel >= MAX_PLUGIN_FUEL_BUDGET {
    tracing::warn!(plugin = plugin_id, "plugin fuel budget exhausted");
    return HookResult::ok(ctx.input);
}
```

### Module Size Limits

Validate WASM module size before compilation:

```rust
if wasm_bytes.len() > MAX_WASM_SIZE {
    tracing::warn!(
        plugin = plugin_id,
        size = wasm_bytes.len(),
        max = MAX_WASM_SIZE,
        "WASM module exceeds maximum size"
    );
    return HookResult::ok(ctx.input);
}
```

### Hook Timeout

Wrap WASM hook execution in a timeout:

```rust
use tokio::time::timeout;

let hook_result = timeout(WASM_HOOK_TIMEOUT, async {
    // Execute hook
}).await;

match hook_result {
    Ok(_) => {}
    Err(_) => {
        tracing::warn!(plugin = plugin_id, "WASM hook timed out after {:?}", WASM_HOOK_TIMEOUT);
    }
}
```

## OAuth State Replay Prevention

Authorization codes can be replayed if not tracked. Use a persistent store for used codes with expiration:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsedCode {
    expires_at: u64,
}

pub struct OAuthManager {
    // ...
    used_codes: std::collections::HashMap<String, UsedCode>,
    used_codes_store: PathBuf,  // Persistent storage path
}

impl OAuthManager {
    fn is_code_used(&self, code: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(used_code) = self.used_codes.get(code) {
            if now < used_code.expires_at {
                return true;
            }
        }
        false
    }

    async fn mark_code_used(&mut self, code: String, expires_at: u64) -> Result<(), McpError> {
        self.used_codes.insert(code, UsedCode { expires_at });
        self.save_used_codes_async().await  // Persist to disk
    }

    fn cleanup_expired_codes(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.used_codes.retain(|_, v| now < v.expires_at);
    }
}
```

**Important**: Used codes must be persisted to survive restarts. Store in `mcp_used_codes.json` alongside token storage.

## Token Encryption

OAuth tokens must be encrypted at rest. Require `OPENCODE_TOKEN_KEY` environment variable or fail to store:

```rust
if let Some(key) = get_encryption_key() {
    // Encrypt and save
} else {
    return Err(McpError::OAuth(
        "cannot save tokens: OPENCODE_TOKEN_KEY environment variable not set".to_string(),
    ));
}
```

## Session Export Secrets Redaction

When exporting sessions, scrub sensitive data from tool calls:

```rust
fn redact_for_export(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(mut obj) => {
            if let Some(serde_json::Value::String(type_str)) = obj.get("type") {
                if type_str == "tool_call" {
                    // Redact input and output
                    if obj.get("input").is_some() {
                        obj.insert("input".to_string(), serde_json::json!("[REDACTED]"));
                    }
                    if let Some(output) = obj.get("output") {
                        if !output.is_null() {
                            obj.insert("output".to_string(), serde_json::json!("[REDACTED]"));
                        }
                    }
                    // Redact sensitive fields for dangerous tools
                    if let Some(serde_json::Value::String(name)) = obj.get("name") {
                        if name == "bash" || name == "write" || name == "read" || name == "edit" {
                            // Redact command, path, content, text fields
                        }
                    }
                }
            }
            // Recurse into nested objects
            for (_, v) in obj.iter_mut() {
                *v = redact_for_export(std::mem::take(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(redact_for_export).collect())
        }
        other => other,
    }
}
```

## Config Endpoint Key Redaction

When returning config via API, redact all sensitive fields:

```rust
fn redact_api_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(mut obj) => {
            let sensitive = ["key", "secret", "password", "token"];
            let keys_to_redact: Vec<String> = obj
                .keys()
                .filter(|k| {
                    let lower = k.to_lowercase();
                    sensitive.iter().any(|s| lower.contains(s))
                })
                .cloned()
                .collect();

            for k in keys_to_redact {
                if let Some(serde_json::Value::String(_)) = obj.get(&k) {
                    obj.insert(k, serde_json::json!("[REDACTED]"));
                }
            }
            // Recurse
            for (_, v) in obj.iter_mut() {
                *v = redact_api_keys(std::mem::take(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(redact_api_keys).collect())
        }
        other => other,
    }
}
```

## Share URL Expiration

Share URLs should expire to prevent indefinite access:

```rust
const SHARE_DURATION_DAYS: i64 = 7;

pub async fn share_session(&self, session_id: &str) -> Result<Session, StorageError> {
    let now = Utc::now().timestamp_millis();
    let share_expires_at = now + (SHARE_DURATION_DAYS * 24 * 60 * 60 * 1000);

    // Generate secure random token
    let mut token_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut token_bytes);
    let token = base64::urlsafe_encode(&token_bytes);

    // Store with expiration
    sqlx::query(
        r#"INSERT INTO session_share (session_id, id, secret, url, share_expires_at, ...)
           VALUES (?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(session_id) DO UPDATE SET ..."#,
    )
    .bind(session_id)
    .bind(&share_id)
    .bind(&token)
    .bind(&url)
    .bind(share_expires_at)
    // ...
}
```

## Plugin Installation Symlink Prevention

When installing plugins, reject symlinks to prevent path traversal:

```rust
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let entries = std::fs::read_dir(src)?;

    for entry in entries {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Reject symlinks
        if ty.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("symlinks are not allowed: {}", src_path.display()),
            ));
        }

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
```

## Landlock Sandboxing

The bash tool supports OS-level filesystem sandboxing via Landlock (Linux 5.13+):

```rust
use crate::security::sandbox::{SandboxConfig, get_default_allowed_paths, get_sensitive_paths};

// Create sandbox configuration
let config = SandboxConfig::new()
    .with_enabled(true)
    .with_allowed_paths(get_default_allowed_paths())
    .with_deny_paths(get_sensitive_paths());

// Enforce sandbox before bash execution
config.enforce()?;
```

Note: `SandboxConfig` is the struct (not `LandlockSandbox`). The struct has builder methods:
- `new()` - creates default config
- `with_enabled(bool)` - enable/disable sandbox
- `with_allowed_paths(Vec<String>)` - set allowed paths
- `with_deny_paths(Vec<String>)` - set denied paths
- `is_available()` - check if Landlock is supported on this system
- `enforce()` - apply the sandbox restrictions

The sandbox uses Linux Landlock syscalls to restrict filesystem access. On unsupported systems, it falls back gracefully.

**Key features:**
- Restricts read/write/exec to allowed paths only
- Denies access to sensitive paths (/etc, /root, /home)
- Uses exponential backoff on syscall failures
- Falls back to path validation if Landlock unavailable

See `.opencode/skills/sandbox/SKILL.md` for full documentation.
