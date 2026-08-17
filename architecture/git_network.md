# Git Network

Network and destructive Git operations with env hardening and credential redaction.

## Purpose

`src/git_network_ops.rs` and `src/git_network_policy.rs` handle network Git operations (fetch, pull, push, remote management) and destructive operations (reset, clean). They extend `GitEnvPolicy` with network-specific environment variables and enforce credential redaction.

## Module Structure

| File | Purpose |
|------|---------|
| `git_network_ops.rs` | Network operation implementations |
| `git_network_policy.rs` | Env policy, credential redaction, failure classification |

## Key Types

### NetworkEnvPolicy

Extends `GitEnvPolicy` with network-specific environment variables:

- Adds SSH agent, proxy, and credential helper env vars
- Preserves 22 allowed network env vars (see `NETWORK_ALLOWED_ENV_VARS`)
- Method: `apply_to_command(argv, cwd) -> Command`

### NETWORK_ALLOWED_ENV_VARS

```rust
&[
    "SSH_AUTH_SOCK", "SSH_AGENT_PID",        // SSH agent
    "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", // Proxy
    "GIT_SSH_COMMAND", "GIT_SSH",            // SSH config
    "GIT_CONFIG_GLOBAL", "GIT_CONFIG_LOCAL",  // Git config
    "GIT_TRACE", "GIT_CURL_VERBOSE",         // Debugging
    // ... 22 total
]
```

### NetworkFailureClass

Heuristic classification of network failures:

| Variant | Meaning |
|---------|---------|
| `Dns` | DNS resolution failure |
| `Connect` | Connection refused/timeout |
| `Authentication` | Auth failure (bad credentials) |
| `Authorization` | Permission denied |
| `RefRejected` | Push rejected (non-fast-forward) |
| `Timeout` | Operation timed out |
| `Transport` | Low-level transport error |

### classify_network_failure()

```rust
fn classify_network_failure(stderr: &str, exit_code: i32, timed_out: bool) -> NetworkFailureClass
```

Heuristic stderr + exit code classifier for network failures.

## NETWORK_DEFAULT_TIMEOUT

```rust
const NETWORK_DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
```

## Credential Redaction

All network operations apply URL credential redaction before logging or returning results. Credentials in remote URLs are replaced with `[REDACTED]`.

## Operations

| Operation | Risk Class | Notes |
|-----------|-----------|-------|
| `git fetch` | Low | Read-only network |
| `git pull` | Medium | Network + merge |
| `git push` | High | Network + write |
| `git push --force` | High | Rejected by tool-side policy |
| `git remote add/remove` | Medium | Remote management |
| `git reset --hard` | High | Destructive local |
| `git clean -f` | High | Destructive local, rejected broadly |

## See Also

- [Git](git.md) — Full Git module overview
- [Git Mutations](git_mutations.md) — Local mutation operations
- [Git Recovery](git_recovery.md) — In-progress operation recovery
- [Security](security.md) — Credential handling and redaction
