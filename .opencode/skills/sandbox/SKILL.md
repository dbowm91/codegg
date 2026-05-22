---
name: sandbox
description: OS-level filesystem sandboxing for bash tool using Landlock
tags: [security, sandbox, landlock, linux, bash]
---

Use the `/skill:sandbox` command to load context about Landlock sandboxing for the bash tool.

## Overview

Landlock provides OS-level filesystem sandboxing for the bash tool on Linux systems. It restricts filesystem access to only allowed paths, preventing accidental or malicious access to sensitive directories.

## Requirements

- Linux kernel 5.13+
- Landlock support enabled in kernel

On unsupported systems, the sandbox falls back gracefully with a warning.

## Configuration

```yaml
security:
  sandbox:
    enabled: true
    allowed_paths:
      - "/home/user/project"
      - "/tmp/opencode"
    deny_paths:
      - "/etc"
      - "/root"
      - "/home"
```

## How It Works

The sandbox uses Linux Landlock syscalls to create filesystem rules:
1. Creates a ruleset with allowed read/write/exec paths
2. Adds rules to deny access to sensitive paths
3. Enforces the ruleset before bash command execution

Key syscalls:
- `SYS_landlock_create_ruleset` - creates the ruleset
- `SYS_landlock_add_rule` - adds path rules
- `SYS_landlock_restrict_self` - enforces the ruleset

## Module

`src/security/sandbox.rs` contains:
- `SandboxConfig` struct for configuration
- `enforce()` method to apply sandbox
- `get_default_allowed_paths()` and `get_sensitive_paths()` helpers

## Fallback Behavior

If Landlock is unavailable:
1. Logs a warning
2. Falls back to path validation in bash tool
3. Continues execution without OS-level enforcement

## Usage

Enable in config:
```yaml
security:
  sandbox:
    enabled: true
```

Or via builder pattern:
```rust
let config = SandboxConfig::new()
    .with_enabled(true)
    .with_allowed_paths(vec!["/project".to_string()])
    .with_deny_paths(vec!["/etc".to_string()]);
config.enforce()?;
```

Note: `SandboxConfig` is the actual struct, not `LandlockSandbox` as shown in older documentation.