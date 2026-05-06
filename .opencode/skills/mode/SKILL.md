---
name: mode
description: Mode system for specialized permission workflows (Review/Debug/Docs)
tags: [mode, permission, workflow, review, debug, docs]
---

Use the `/skill:mode` command to load context about the mode system for specialized workflows.

## Overview

The mode system provides specialized permission configurations for different technical workflows. Modes can be switched at runtime or configured per-session.

## Builtin Modes

### Review Mode
- **Purpose**: Code review workflows
- **Default**: Ask
- **Tools**: read, glob, grep allowed; bash, edit denied

### Debug Mode
- **Purpose**: Debugging sessions
- **Default**: Allow
- **Tools**: bash allowed, read allowed, edit ask

### Docs Mode
- **Purpose**: Documentation work
- **Default**: Allow
- **Tools**: edit, read allowed; bash denied

## Configuration

```yaml
mode:
  review:
    description: "Code review mode"
    default: "ask"
    inherit: true
    tools:
      read: "allow"
      glob: "allow"
      grep: "allow"
      bash: "deny"
      edit: "deny"
  debug:
    description: "Debug mode"
    default: "allow"
    tools:
      bash: "allow"
      read: "allow"
      edit: "ask"
```

## Mode Inheritance

Modes can inherit from base permission configuration:
- `inherit: true` - start with default permissions, override with mode tools
- `inherit: false` - use only mode-specific permissions

## Module

`src/permission/modes.rs` contains:
- `ModeConfig` for configuration
- `BuiltinModes` for predefined modes
- `mode_ruleset()` to generate permission ruleset from mode

## Usage

```rust
// Get builtin mode
let review = get_builtin_mode("review").unwrap();

// Create from config
let ruleset = mode_ruleset(&config_mode, base_ruleset);
```

## Agent Integration

Agents can specify a mode:
```rust
let agent = Agent {
    name: "reviewer".to_string(),
    mode_name: Some("review".to_string()),
    ..Default::default()
};
```