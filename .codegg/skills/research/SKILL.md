---
name: research
description: Deep research tool and `research` agent kind for multi-source synthesis
version: 1.0.0
tags:
  - research
  - subagent
  - websearch
  - synthesis
---

# Research Tool and Subagent

The `research` tool provides deep, multi-source research that goes beyond a single `websearch` query. It is paired with a `research` agent kind that the main agent can spawn via the `task` tool for delegated long-horizon research work.

## Components

| File | Purpose |
|------|---------|
| `src/tool/research.rs` | `ResearchTool` — user-facing tool that wraps `ResearchService` |
| `src/agent/mod.rs` (`builtin_agents`) | Defines the `research` agent kind (`AgentMode::All`) |
| `src/agent/prompt.rs` | `research_subagent_contract()` and `role_contract("researcher")` |
| `src/config/schema.rs` | `ResearchAutoTriggerConfig { enabled, min_confidence }` |
| `src/agent/loop.rs` | `maybe_inject_research_hint` — trigger hint injection into first user message |

## `ResearchTool`

The tool is registered in `ToolRegistry::with_defaults()` and `ToolRegistry::with_session_defaults()`.

```rust
pub struct ResearchTool {
    service: Arc<ResearchService>,
}

impl ResearchTool {
    pub fn with_default_service() -> Self {
        // Service is rooted at current_dir() and shared via a Lazy<Arc<...>>
    }
}
```

Parameters:
- `question: String` (required) — what to research.
- `mode: ResearchMode` (default: `Quick`) — `Quick | Standard | Deep`.
- `depth: Option<usize>` — optional cap on number of sub-queries.

The tool delegates to `ResearchService::answer_for_agent` which orchestrates `websearch`/`webfetch` calls and produces a synthesized answer. The output is returned to the main agent as a `ToolOutput`.

## `research` Agent Kind

```rust
Agent {
    kind: AgentKind::Builtin("research"),
    mode: AgentMode::All,  // user-spawnable AND main-agent-spawnable
    role: "researcher",
    color: Color::Magenta,
    system_prompt: /* instructs the agent on research discipline */,
    permissions: /* read-mostly; network allowed; writes ask */,
    ..
}
```

It is **not** duplicated as a separate main agent kind — `AgentMode::All` is sufficient for both surfaces (user `/agents` and main-agent `task` tool).

### Default Permissions

| Action | Tools |
|--------|-------|
| **Allow** | `read`, `glob`, `grep`, `list`, `websearch`, `webfetch`, `research`, `todowrite`, `todoread`, `skill`, `question`, `task` |
| **Ask** | `bash`, `edit`, `write`, `multiedit`, `apply_patch`, `terminal`, `commit` |
| **Deny** | `image` |

These mirror the existing `general` subagent policy, except `research` is explicitly in the allow list so the subagent can call itself recursively for sub-questions.

## Prompt Contracts

`assemble_system_prompt_with_profile` adds two contracts when relevant:

1. **`websearch_contract()`** — appended whenever the `websearch` tool is in the active toolset. Tells the main agent to prefer `websearch` over `curl`/`bash` and to use `webfetch` for full-page reads.

2. **`research_subagent_contract()`** — appended whenever at least one subagent is registered. Tells the main agent that `task` tool can spawn `agent: "research"` for deep research work, and that the result is a synthesized answer (not raw hits).

The `role_contract` function (in `src/agent/prompt.rs`) has a `researcher` arm that returns research-discipline instructions (cite sources, prefer primary over secondary, surface uncertainty).

## Auto-Trigger Heuristic

`AgentLoop::maybe_inject_research_hint(&self, user_prompt)` runs on the first user turn:

```rust
pub fn maybe_inject_research_hint(&self, user_prompt: &str) -> Option<String> {
    let cfg = &self.research_config.auto_trigger;
    if !cfg.enabled { return None; }
    if self.plan_mode { return None; }
    let analysis = triggers::analyze_trigger(user_prompt, cfg.min_confidence);
    if !analysis.should_trigger { return None; }
    Some(format!(
        "[system hint: user query looks like a research question (mode={:?}, confidence={:.2}). \
         Consider spawning a `research` subagent via the `task` tool for a synthesized answer.]",
        analysis.mode, analysis.confidence
    ))
}
```

The hint is prepended to the first user message as a `ContentPart::Text` segment. The main agent sees a *recommendation*, not a forced re-route — it can still answer directly or use `websearch` itself.

The heuristic looks for signals like:
- Comparative phrasing ("compare X vs Y", "X or Y").
- Multi-hop questions ("history of", "how does X work", "explain X in detail").
- Explicit research verbs ("research", "investigate", "analyze").
- Quantitative asks ("top 10", "best practices").

Trigger logic lives in `src/agent/loop.rs` (`mod tests` block) with tests like `research_trigger_fires_on_comparison_query`.

## Config Schema

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ResearchAutoTriggerConfig {
    pub enabled: bool,         // default: true
    pub min_confidence: f32,   // default: 0.7
}
```

This is nested under `ResearchConfig` in `config.json`:

```json
{
  "research": {
    "auto_trigger": {
      "enabled": true,
      "min_confidence": 0.7
    }
  }
}
```

## Testing

```bash
# Tool tests
cargo test --lib tool::research

# Agent kind registration
cargo test --lib agent::tests::test_builtin_research_agent_registered

# Subagent registry includes websearch + research
cargo test --lib agent::tests::test_research_subagent_registry_includes_websearch_and_research

# Prompt contracts
cargo test --lib agent::prompt::test_websearch_contract
cargo test --lib agent::prompt::test_research_subagent_contract

# Trigger heuristic
cargo test --lib agent::r#loop::research_trigger
```

## Future Work

- Replace `Lazy<Arc<ResearchService>>` with explicit wiring so tests can inject a mock service.
- Add a streaming variant of `ResearchService` for long-running research queries.
- Expose `ResearchConfig` in the TUI settings dialog.
- Add `/research <question>` slash command for direct user invocation.
