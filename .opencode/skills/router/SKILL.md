---
name: router
description: Model auto-routing system for task complexity-based model selection
version: 1.0.0
tags: [agent, router, model, complexity]
---

# Model Router Guide

This skill covers the `ModelRouter` in `src/agent/router.rs` which automatically routes tasks to appropriate LLM models based on task complexity.

## Overview

The `ModelRouter` enables `auto_route_models: true` in config to automatically select faster/cheaper models for simple tasks while reserving more capable models for complex tasks.

## TaskComplexity Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskComplexity {
    Simple,   // Fast operations like reading, listing files
    Medium,   // Standard operations like editing, writing
    Complex,  // Complex operations like debugging, planning, architecture
}
```

## ModelRouter Struct

```rust
#[allow(dead_code)]
pub struct ModelRouter {
    enabled: bool,
    simple_model: Option<String>,   // e.g., gpt-4o-mini
    medium_model: Option<String>,   // Current model (default)
    complex_model: Option<String>,  // Current model (default)
}
```

## Classification

The router classifies tasks using two methods:

### By Tool Name

```rust
fn classify_by_tool(&self, tool_name: &str) -> TaskComplexity {
    match tool_name {
        "read" | "cat" | "ls" | "glob" | "list" => TaskComplexity::Simple,
        "edit" | "write" | "grep" | "search" => TaskComplexity::Medium,
        "debug" | "plan" | "review" | "architect" | "analyze" => TaskComplexity::Complex,
        _ => TaskComplexity::Medium,
    }
}
```

### By Content Keywords

```rust
fn classify_by_content(&self, prompt: &str) -> TaskComplexity {
    // Complex keywords: debug, analyze, plan, architect, review, design, optimize,
    //                    refactor, investigate, troubleshoot, complex, difficult,
    //                    understand the codebase, architecture, performance issue
    // Medium keywords: edit, write, create, modify, change, update, add, fix,
    //                  implement, function, feature, improve
    // Simple keywords: read, show, list, find, get, look, view, display,
    //                   what is, cat, ls, glob, grep, search
}
```

Classification rules:
- **Complex**: >=2 complex keywords OR contains "debug this" OR contains "analyze the"
- **Medium**: 1 complex keyword OR >=2 medium keywords
- **Simple**: >=2 simple keywords OR prompt.len() < 50
- **Default**: Medium

## Main Classification Method

```rust
pub fn classify(&self, prompt: &str, tool_name: &str) -> TaskComplexity {
    // Tool-based classification takes precedence for Complex
    if self.classify_by_tool(tool_name) == TaskComplexity::Complex {
        return TaskComplexity::Complex;
    }

    // Then use content-based classification
    self.classify_by_content(prompt)
}
```

## Routing

```rust
pub fn route_model(&self, complexity: TaskComplexity) -> Option<String> {
    if !self.enabled {
        return None;
    }

    match complexity {
        TaskComplexity::Simple => self.simple_model.clone(),
        TaskComplexity::Medium => None,  // Keep current model
        TaskComplexity::Complex => None, // Keep current model
    }
}
```

**Note**: Currently only `Simple` tasks are routed to a different model. Medium and Complex tasks use the configured default model.

## Configuration

```rust
pub struct Config {
    pub auto_route_models: Option<bool>,  // Enable/disable routing
    pub small_model: Option<String>,       // Model for simple tasks
    pub medium_model: Option<String>,      // Model for medium tasks (uses `model` field)
    pub model: Option<String>,             // Default model (also used for complex)
}
```

Enable in config.yaml:
```yaml
auto_route_models: true
small_model: gpt-4o-mini  # Or claude-3-haiku, etc.
```

## Usage in AgentLoop

The ModelRouter is initialized in `AgentLoop::new()`:

```rust
let model_router = ModelRouter::from_config(&config);
```

And used during message processing to potentially route to a different model for simple tasks.

## Related Skills

- See `.opencode/skills/agent-loop/SKILL.md` for AgentLoop integration
- See `.opencode/skills/provider/SKILL.md` for provider implementation