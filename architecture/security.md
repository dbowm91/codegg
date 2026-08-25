# Security Module

The `security` module provides URL validation, IP checking, filesystem
sandboxing, security scanning, command classification, and an
evidence-based security review workflow.

## Purpose

Prevent SSRF attacks, confine subprocess filesystem access via Landlock,
classify shell commands and tool calls by risk, scan code for secrets
and unsafe patterns, and run a conservative evidence-based security
review that never mutates files.

## Where It Lives

| Artifact | Location |
|----------|----------|
| SSRF protection (`is_internal_ip`, `validate_url_host`, etc.) | `src/security/ssrf.rs` |
| Landlock sandboxing, `SandboxConfig`, `SandboxLaunchSpec` | `src/security/sandbox.rs` |
| Security policy (`action_for_command`, `action_for_findings`) | `src/security/policy.rs` |
| High-level `SecurityService` facade | `src/security/service.rs` |
| Security review workflow (diff parsing, evidence synthesis) | `src/security/workflow/` |
| Security review runtime (`prepare_security_review`, `validate_report`) | `src/security/runtime.rs` |
| LSP security context executor adapter | `src/security/lsp_executor.rs` |
| Bounded HTTP body reader | `src/security/untrusted_http.rs` |
| Sensitive path matching | `src/security/mod.rs` (`matches_sensitive_path`) |
| Re-exports of eggsentry types for backward compat | `src/security/mod.rs` |
| Deterministic security scanning (secrets, commands, deps) | `crates/eggsentry/src/` |
| `codegg-sandbox-helper` binary (Linux Landlock enforcement) | `src/bin/codegg-sandbox-helper/` |

## How It Works

### SSRF Protection (`ssrf.rs`)

Prevents requests to internal infrastructure via a multi-stage
validation pipeline:

1. **`validate_url_target(raw_url)`** (`ssrf.rs:129`) — parses URL,
   checks scheme (http/https only), resolves DNS once, validates all
   resolved addresses. Returns `ValidatedUrlTarget` with pinned
   `SocketAddr` set for `reqwest::ClientBuilder::resolve_to_addrs`.
2. **`validate_url_host(url)`** (`ssrf.rs:182`) — convenience wrapper
   returning just the normalized host string.
3. **`validate_host_ip(host, port)`** (`ssrf.rs:88`) — DNS resolution +
   internal IP check on all resolved addresses.
4. **`revalidate_dns(host, port, validated_ips)`** (`ssrf.rs:155`) —
   re-resolves DNS and verifies IPs haven't changed (DNS rebinding
   protection). Handles IPv4-mapped IPv6 equivalence.
5. **`is_internal_ip(ip)`** (`ssrf.rs:25`) — checks against all
   reserved ranges (loopback, private, link-local, multicast, CGNAT,
   benchmark, IPv4-mapped IPv6, IPv6 unique local, etc.).
6. **`ipv6_segments_to_ipv4(ipv6)`** (`ssrf.rs:60`) — converts
   IPv4-mapped and IPv4-compatible IPv6 to IPv4 for range checking.

Used by:
- `tool::webfetch::execute_builtin` — the `builtin` webfetch path
- `src/mcp/remote.rs` — MCP remote URL validation
- The default `eggsearch` backend delegates SSRF protection to the
  eggsearch subprocess; Codegg only does basic URL validation
  (`validate_fetch_url`) on the adapter path.

### Landlock Sandboxing (`sandbox.rs`)

Linux enforcement is via the maintained `landlock` crate, performed
**only** by the private one-shot `codegg-sandbox-helper` process. The
daemon never calls `restrict_self()`.

**Architecture:**
- Parent resolves helper as canonical regular-file sibling of the
  running binary (inherited env/PATH/cwd cannot select it)
- Parent serializes a bounded `SandboxLaunchSpec` to an owner-only
  ephemeral file in the system temp directory
- Helper adds every Landlock rule, requires `FullyEnforced` +
  `no_new_privs`, writes a versioned typed status frame to a private
  pipe
- Helper marks the status writer close-on-exec before exec'ing the
  target
- Parent accepts only the expected setup/exec state sequence; fails
  closed on malformed, duplicate, oversized, or contradictory frames

**Key types:**
```rust
pub enum SandboxMode { ReadOnly, WorkspaceWrite, DangerFullAccess }

pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,  // retained for compat, not enforced as zero-access rule
}

pub struct SandboxLaunchSpec {
    pub target: PathBuf,
    pub args: Vec<String>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
}
```

`SandboxConfig::enforce()` refuses to restrict the calling process —
callers must use the child launch path.

**Platform outcomes:**
- Linux with Landlock ABI: `Enforced { abi }` (ABI V1 minimum)
- Non-Linux or no Landlock: Python portable fallback with sanitized
  environment, workspace-contained cwd, snapshot-based post-exec checks

**CANONICAL_PATHS_CACHE:** Static cache with 300s TTL and 100-entry
cap (`sandbox.rs:453-458`). Entries older than 300s are evicted on
access.

### Security Policy (`policy.rs`)

Decides `Observe | Ask | Deny` based on `CommandClassification` and
`SecurityConfig`:

```rust
pub enum SecurityAction { Observe, Ask, Deny }

pub struct SecurityDecisionHint {
    pub action: SecurityAction,
    pub reason: String,
    pub finding: Option<SecurityFinding>,
}
```

**Decision logic** (`action_for_command`):
1. Disabled/off → `Observe`
2. Explicit deny list match → `Deny`
3. Review mode: Critical/High risk → `Ask` (never Deny); else `Observe`
4. Critical + `deny_critical_commands` → `Deny`
5. Critical + `ask_on_high_risk_command` → `Ask`
6. Network exfiltration category → `Ask`
7. Secret exposure category → `Ask`
8. High risk + `ask_on_high_risk_command` → `Ask`
9. Strict mode + Medium risk → `Ask`
10. Otherwise → `Observe`

`action_for_findings` uses the worst-severity finding with similar
logic. Review mode always observes (reports findings but never denies).

### SecurityService (`service.rs`)

High-level facade used by the agent loop:

```rust
pub struct SecurityService { config: SecurityConfig }
```

Methods: `classify_tool_call(name, args)`, `classify_bash(command)`,
`classify_git(subcommand)`, `classify_raw(classification)`,
`format_prompt_hints(findings)`.

### Security Review Workflow (`workflow/`)

A read-only, evidence-based security review pipeline:

1. **Diff parsing** (`diff.rs`) — parses unified diffs into
   `ChangedHunk` structs with file paths and line ranges
2. **Preset selection** (`enrichment.rs`) — selects review presets
   (dependency_review, unsafe_review, web_backend, rust_cli,
   rust_server) based on file path and content
3. **Target building** — deduplicates review targets from hunks,
   excludes vendor/target/node_modules/third_party/dist/build
4. **Preflight checks** (`preflight.rs`) — deterministic checks
   (secret filename hints, content scans)
5. **Evidence synthesis** (`evidence.rs`) — conservative: risk markers
   alone never produce findings; requires 2+ evidence dimensions
   (e.g., RiskMarker + ChangedHunk, or Preflight + ChangedHunk)
6. **Report assembly** (`report.rs`) — produces `SecurityReviewReport`
   with targets, prompts, findings, and notes

Key invariants:
- Findings are defensive review outputs, never proof of exploitability
- Same-file scoping only
- Severity/confidence are deterministic enums
- Malformed or oversized model output is rejected; unsupported
  findings become evidence gaps

### eggsentry Crate (`crates/eggsentry/`)

Deterministic security scanning primitives:

| Module | Purpose |
|--------|---------|
| `command.rs` | `classify_bash_command`, `classify_git_subcommand`, `classify_tool_call` — risk classification with regex patterns |
| `scanner.rs` | `inspect_text`, `inspect_file` — secret/unsafe-code pattern scanning with 15+ regex rules |
| `finding.rs` | `SecurityFinding`, `SecurityReport`, `Severity`, `Confidence`, `SecurityCategory` — 16 categories |
| `profile.rs` | `ProfileRunner` with 4 profiles: Ambient, DependencyDelta, PreCommit, SecurityReview |
| `dependency.rs` | `detect_dependency_file`, `recommended_audit_commands` — 7 ecosystems |

**Secret detection patterns** (scanner.rs): AWS keys, GitHub tokens
(ghp/gho/ghu/ghs/ghr/github_pat), Slack tokens, npm tokens, PyPI
tokens, GCP service accounts, OpenAI keys, private key blocks,
passwords, API keys, secrets. Also scans for unsafe blocks, unsafe
functions, `danger_accept_invalid_certs`, `Command::new("sh"/"bash")`,
CORS wildcards, and bind-all addresses.

**Command risk levels** (command.rs):
- Critical: `rm -rf /`, fork bombs, `mkfs`, `dd of=/dev/`,
  `shutdown`, `chmod 777 /`, curl-pipe-sh, private key piping
- High: `git push --force`, `git reset --hard`, `git clean -fdx`,
  docker privileged/root-mount/socket, kubectl/terraform/ansible,
  scp/rsync/netcat/ftp, ssh remote exec, env exfiltration
- Medium: `rm`, `mv`, `cp`, package manager installs, env dumps,
  chmod, sed -i, perl -pi, git push
- Low: cargo test/check/build/clippy, git read-only, ls, cat, grep

### Sensitive Path Matching (`mod.rs:65`)

```rust
pub fn matches_sensitive_path<'a>(
    file_path: Option<&str>,
    sensitive_paths: &'a [SensitivePathConfig],
) -> Option<&'a SensitivePathConfig>
```

Matches file paths against configured glob patterns with
canonicalization.

### Untrusted HTTP (`untrusted_http.rs`)

```rust
pub(crate) async fn read_body_bounded(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, BoundedBodyError>
```

Enforces response body limits on both `Content-Length` and streamed
chunks. Prevents unbounded memory allocation from untrusted HTTP
responses.

## Key Types & APIs

### SSRF (`ssrf.rs`)

- `is_internal_ip(ip: &IpAddr) -> bool` (:25)
- `ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr>` (:60)
- `validate_host_ip(host, port) -> Result<Vec<IpAddr>, String>` (:88)
- `validate_url_target(raw_url) -> Result<ValidatedUrlTarget, String>` (:129)
- `validate_url_host(url) -> Result<String, String>` (:182)
- `revalidate_dns(host, port, validated_ips) -> Result<(), String>` (:155)

### Sandbox (`sandbox.rs`)

- `SandboxConfig::enforce()` — refuses to restrict parent (:76)
- `SandboxConfig::launch_spec(target, args, cwd)` — builds child
  launch description (:89)
- `sandbox_helper_path()` — resolves trusted helper binary (:290)
- `validate_path_safety(path, allowed_paths)` — symlink check +
  canonicalization (:509)
- `probe_landlock()` — checks Landlock ABI availability (:350)
- `apply_landlock(spec)` — applies Landlock rules (Linux only) (:367)

### Policy (`policy.rs`)

- `action_for_command(classification, config) -> SecurityDecisionHint` (:23)
- `action_for_findings(findings, config) -> SecurityDecisionHint` (:189)

### Runtime (`runtime.rs`)

- `prepare_security_review(input) -> Result<(SecurityEvidenceBundle, SecurityReviewOutput), String>` (:119)
- `validate_report(report, bundle) -> (SecurityReviewReport, Vec<String>)` (:209)

### eggsentry (`crates/eggsentry/src/`)

- `classify_bash_command(cmd) -> CommandClassification` (command.rs:193)
- `classify_git_subcommand(sub) -> CommandClassification` (command.rs:503)
- `classify_tool_call(name, args) -> CommandClassification` (command.rs:507)
- `inspect_text(path, text) -> Vec<SecurityFinding>` (scanner.rs:308)
- `inspect_file(path, max_bytes) -> Result<Vec<SecurityFinding>>` (scanner.rs:391)
- `ProfileRunner::inspect_paths(profile, paths) -> SecurityReport` (profile.rs:48)

## Configuration Surface

Security configuration lives in `SecurityConfig` (from
`config::schema`):

```toml
[security]
enabled = true
mode = "ambient"           # Off | Ambient | Strict | Review
prompt_hints = false
max_findings_in_prompt = 10
denied_commands = []       # explicit deny list

[security.gates]
deny_critical_commands = true
ask_on_high_risk_command = true
ask_on_network_exfiltration = true
ask_on_secret_exposure = true
```

**Security modes:**
- `Off` — all checks return Observe
- `Ambient` — observation only, no auto-deny
- `Strict` — Medium-risk commands also Ask
- `Review` — Critical/High Ask (never Deny); everything else Observe

Tool backend configuration:

```toml
[tool_backends.security]
backend = "native"         # native | mcp | disabled
fallback_to_native = true  # only for mcp backend
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
| `fe80::/10` | IPv6 link-local |
| `ff00::/8` | IPv6 multicast |
| `::ffff:x.x.x.x` | IPv4-mapped IPv6 |

## Invariants & Gotchas

1. **Sandbox is child-process-only.** `SandboxConfig::enforce()`
   returns an error if called on an enabled config. The daemon is never
   confined.
2. **Security review never mutates files.** The workflow is read-only;
   findings are defensive outputs, not proof of exploitability.
3. **Finding synthesis requires 2+ evidence dimensions.** Risk markers
   alone produce review prompts, never findings. Same-file scoping only.
4. **DNS rebinding protection.** `revalidate_dns` re-resolves and
   compares; IPv4-mapped IPv6 equivalence is handled.
5. **Bounded HTTP body.** `read_body_bounded` checks both
   Content-Length and streamed chunks against the limit.
6. **eggsentry is deterministic.** No network calls, no file mutations.
   Regex-based scanning with `LazyLock` compiled patterns.
7. **Landlock ABI V1 minimum.** The helper requires `FullyEnforced` +
  `no_new_privs`; partial enforcement is rejected.

## Testing

```bash
# SSRF tests
cargo test -p codegg --lib security::ssrf

# Sandbox tests (includes Landlock-specific tests on Linux)
cargo test -p codegg --lib security::sandbox

# Policy tests
cargo test -p codegg --lib security::policy

# Service tests
cargo test -p codegg --lib security::service

# Workflow tests (diff parsing, evidence synthesis)
cargo test -p codegg --lib security::workflow

# Runtime tests (prepare_security_review, validate_report)
cargo test -p codegg --lib security::runtime

# eggsentry tests (scanner, command classification, profiles)
cargo test -p eggsentry

# Untrusted HTTP tests
cargo test -p codegg --lib security::untrusted_http
```

## Related Docs

- [tool.md](tool.md) — Uses security validation
- [permission.md](permission.md) — Path permissions
- [native_crates.md](native_crates.md) — eggsentry crate details
