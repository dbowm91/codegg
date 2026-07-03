# Agent Customization Milestone 4: Agent Registry and Resolution Model

## Goal

Introduce a central `AgentRegistry` that separates declarative agent sources from resolved runtime agents. This creates the foundation for safe customization, source-aware diagnostics, and future TUI management commands.

This milestone should preserve existing behavior while replacing scattered resolution logic with a single registry path.

## New concepts

Add or equivalent types:

```rust
pub struct AgentSpec {
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub mode: Option<AgentMode>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub permission: Option<AgentPermissionSpec>,
    pub options: BTreeMap<String, toml::Value>,
}

pub struct ResolvedAgent {
    pub agent: Agent,
    pub sources: Vec<AgentSource>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

pub struct AgentSource {
    pub kind: AgentSourceKind,
    pub path: Option<PathBuf>,
    pub name: String,
}

pub enum AgentSourceKind {
    Builtin,
    GlobalFile,
    ProjectFile,
    ConfigAgent,
    ConfigMode,
    Session,
}
```

The exact field names can adapt to current code style, but the registry must retain source provenance.

## Registry API

Add:

```rust
pub struct AgentRegistry {
    resolved: BTreeMap<String, ResolvedAgent>,
    diagnostics: Vec<AgentDiagnostic>,
}

impl AgentRegistry {
    pub fn load(config: &Config) -> Result<Self>;
    pub fn get(&self, name: &str) -> Option<&ResolvedAgent>;
    pub fn list(&self) -> impl Iterator<Item = &ResolvedAgent>;
    pub fn list_visible(&self) -> Vec<&ResolvedAgent>;
    pub fn list_primary(&self) -> Vec<&ResolvedAgent>;
    pub fn list_spawnable(&self) -> Vec<&ResolvedAgent>;
    pub fn diagnostics(&self) -> &[AgentDiagnostic];
    pub fn source_stack(&self, name: &str) -> Option<&[AgentSource]>;
}
```

Keep compatibility helpers around existing call sites:

```rust
pub fn resolve_agents(config: &Config) -> Vec<Agent>;
pub fn builtin_agents() -> Vec<Agent>;
```

Internally, those helpers should delegate to the registry where practical.

## Resolution order

The registry should apply layers in this order:

```text
1. compiled generated built-ins
2. global user agent files
3. project agent files
4. config.agent overrides
5. config.mode compatibility overrides
6. session/runtime safety envelope, when available
```

Session/runtime safety may remain outside the registry initially if that matches current architecture. The registry should still be designed so a later caller can apply it before execution.

## Compatibility phase

For this milestone, do not require every caller to consume `ResolvedAgent`. It is acceptable for existing code to continue using `Vec<Agent>` while new code/tests exercise `AgentRegistry`.

The migration path should be:

1. Add registry and tests.
2. Make existing resolution function call registry.
3. Gradually convert call sites that need diagnostics/source stacks.
4. Leave simple callers on `Agent` until later.

## Diagnostics

Define diagnostics with severity:

```rust
pub enum AgentDiagnosticSeverity {
    Info,
    Warning,
    Error,
}
```

Initial diagnostics should cover:

- Duplicate custom agent names.
- Invalid modes.
- Unknown permissions.
- Missing prompt files.
- Overlay attempts against missing targets.
- Disabled agents.
- Built-in replacement attempts without explicit `replace = true`, once overlays exist.

Built-in generation errors remain generator failures. Registry diagnostics are mostly for user/project/config sources.

## Tests

Add tests for:

- Registry loads compiled built-ins.
- Registry returns visible agents.
- Registry returns primary/all selectable agents.
- Registry returns subagent/all spawnable agents.
- Source stack for built-ins includes `Builtin`.
- Existing `resolve_agents(config)` produces equivalent agent names/modes to prior behavior.

## Acceptance criteria

- `AgentRegistry` exists and can load compiled built-ins.
- Existing agent resolution behavior is preserved.
- Compatibility wrappers keep current call sites stable.
- Source provenance exists for resolved agents.
- Diagnostics can be collected without panicking.
- Tests cover built-in loading, visibility, selection, spawnability, and source stack behavior.

## Handoff notes

This is an architecture milestone. Avoid changing customization semantics here beyond what is required to represent source stacks. The next milestone should add TOML/Markdown user and project agents on top of the registry.
