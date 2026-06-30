# Phase 9 Plan: Core Lifecycle Hook Integration

## Objective

Wire plugin lifecycle hooks into the core execution paths where they actually matter: provider/auth resolution, tool definition generation, tool execution before/after, chat params/headers, message transforms, session compaction, shell env, text completion, config, and event publication.

Hooks should run through the unified plugin service/runtime model from Phases 6-8. They should not be a TUI-only feature.

## Current State

The old plugin system already defines hook types and `PluginService` convenience methods, but earlier inspection showed limited active runtime integration. The post-Phase-5 registry can now index hook capabilities. After Phases 6-8, the service should be able to dispatch hooks across builtin, process, and WASM runtimes through one invocation/response envelope.

This phase makes those hooks operational in core paths.

## Design Rule

Start conservative.

Observation and post-action hooks are lower risk than blocking/mutating hooks. Mutating hooks need strict input/output contracts and capability gates. Blocking hooks need explicit permission/trust treatment.

Recommended rollout order:

1. Event/observation hooks.
2. Post-tool hooks.
3. Shell env hooks.
4. Message transform hooks.
5. Tool definition hooks.
6. Chat params/headers hooks.
7. Pre-tool blocking/mutating hooks.
8. Compaction hooks.
9. Provider/auth hooks.

The final order can differ if the current codebase has clearer seams elsewhere, but do not start by allowing arbitrary pre-tool blocking hooks without policy enforcement.

## Files to Inspect First

Before implementation, inspect current seams in:

- `src/core/daemon.rs`
- `src/agent/loop.rs`
- `src/tool/*`
- `src/provider/*`
- `src/shell/*`
- `src/session/*`
- `src/compaction/*` or wherever compaction is implemented
- `src/bus/events.rs`
- `src/core/mod.rs`

Identify where requests, tools, messages, shell env, and events flow. Prefer narrow integration helpers over large call-site patches.

## Files to Add

### `src/plugin/lifecycle.rs`

Add lifecycle integration helpers that keep hook call sites concise.

Recommended shape:

```rust
pub struct LifecycleHooks {
    service: Arc<PluginService>,
    policy: PluginLifecyclePolicy,
}

impl LifecycleHooks {
    pub async fn emit_event(&self, event: serde_json::Value) -> PluginHookOutcome<serde_json::Value>;
    pub async fn after_tool_execute(&self, input: ToolAfterHookInput) -> PluginHookOutcome<ToolAfterHookOutput>;
    pub async fn before_tool_execute(&self, input: ToolBeforeHookInput) -> PluginHookOutcome<ToolBeforeHookOutput>;
    pub async fn transform_messages(&self, input: MessageTransformInput) -> PluginHookOutcome<MessageTransformOutput>;
    pub async fn shell_env(&self, input: ShellEnvHookInput) -> PluginHookOutcome<ShellEnvHookOutput>;
}
```

Do not put all serialization logic directly into the agent loop or tool runner. Centralize conversion between typed inputs and `PluginInvocation`.

### `src/plugin/policy.rs`

If not already present from Phase 6/8, add a basic lifecycle policy module.

Recommended initial policy:

```rust
pub struct PluginLifecyclePolicy {
    pub enable_observation_hooks: bool,
    pub enable_mutating_hooks: bool,
    pub enable_blocking_hooks: bool,
    pub allow_process_lifecycle_hooks: bool,
}
```

Defaults should be conservative:

- observation hooks enabled only if plugin system enabled;
- mutating/blocking hooks disabled or WASM/builtin only;
- process lifecycle hooks disabled by default.

Process plugins are local executable code and should not silently intercept core lifecycle paths.

## Hook Input/Output Contracts

Define explicit JSON contracts, even if represented as Rust structs internally.

### Event hook

Input:

```json
{
  "event_type": "session.created",
  "event": { ... },
  "session_id": "..."
}
```

Output:

- diagnostics only;
- no mutation;
- no blocking.

### Tool execute before

Input:

```json
{
  "tool_name": "edit",
  "tool_call_id": "...",
  "args": { ... },
  "session_id": "...",
  "risk": "normal"
}
```

Output:

```json
{
  "action": "allow" | "deny" | "modify",
  "args": { ... },
  "reason": "..."
}
```

Policy:

- deny/modify require mutating/blocking capability;
- process runtime should be denied unless explicitly allowed;
- hook diagnostics should be logged.

### Tool execute after

Input:

```json
{
  "tool_name": "edit",
  "tool_call_id": "...",
  "args": { ... },
  "success": true,
  "output": "...",
  "duration_ms": 123
}
```

Output:

- diagnostics;
- optional additional UI/event effects;
- optional output annotation if policy allows.

### Message transform

Input:

```json
{
  "messages": [ ... ],
  "session_id": "...",
  "model": "...",
  "agent": "..."
}
```

Output:

```json
{
  "messages": [ ... ]
}
```

Policy:

- mutating hook only;
- WASM/builtin preferred;
- process disabled by default;
- must preserve valid provider message schema.

### Shell env

Input:

```json
{
  "command": "...",
  "cwd": "...",
  "base_env_keys": ["PATH", "..."]
}
```

Output:

```json
{
  "env": {"KEY": "VALUE"},
  "remove": ["KEY"]
}
```

Policy:

- values should be explicit;
- no secret injection unless permission grants it;
- log keys, not secret values.

## Service API Requirements

Update or add service methods using protocol response types:

```rust
pub async fn dispatch_lifecycle_hook(
    &self,
    hook_type: HookType,
    input: serde_json::Value,
    context: PluginContext,
) -> Result<PluginResponse, PluginError>;
```

For hook chains:

- each hook receives current input;
- a failed hook follows policy: fail-open for observation, fail-closed only for explicitly blocking security hooks if configured;
- transformed output becomes next hook input;
- diagnostics are accumulated and logged;
- UI effects are emitted through event path in Phase 10 or returned to caller for TUI-local display.

## Core Integration Points

### Event bus / CoreDaemon

Wire observation hooks to event publication. This should be non-blocking or time-bounded. Hook failures should not break event publication.

### Tool execution

Wire pre/post hooks around the central tool execution point, not every individual tool. The central tool runner should produce one pre-hook input and one post-hook input.

### Shell execution

Wire shell env hook before spawning human shell or plugin process only if policy allows. Be careful not to let plugin hooks recursively affect plugin process execution unless explicitly intended.

### Message/request construction

Wire message transform before provider request creation, after deterministic context packing. Mutating output must be schema-validated before use.

### Provider params/headers/auth

Leave provider/auth hooks until the end of the phase unless the seam is already clean. These are high-risk because mistakes can break model access or leak credentials.

## Tests

Add focused tests around the lifecycle layer, preferably with a mock runtime/service:

- event hook observes without mutation;
- post-tool hook diagnostics are logged/returned;
- pre-tool hook allow passes args unchanged;
- pre-tool hook deny blocks only when blocking hooks are enabled;
- pre-tool hook modify is ignored when mutating hooks are disabled;
- process lifecycle hook is denied by default;
- WASM/builtin mutating hook is allowed when policy enables it;
- hook timeout follows fail-open/fail-closed policy;
- message transform validates output schema;
- shell env hook does not log secret values.

Add integration tests for at least one real core path:

- a post-tool hook is called from the central tool execution path;
- an event hook is called when a core event is published;
- disabled plugins do not receive lifecycle hooks.

## Telemetry and Diagnostics

Add structured tracing fields:

- plugin id;
- hook type;
- runtime kind;
- duration;
- policy decision;
- timeout/error;
- whether output was mutated or blocked.

Do not log full message content, tool arguments, environment values, or secrets unless debug/lossless mode explicitly permits it.

## Acceptance Criteria

- Lifecycle hooks run from core paths, not only TUI command paths.
- At least event and post-tool hooks are operational through `PluginService`.
- Mutating/blocking hooks are policy-gated.
- Process lifecycle hooks are denied by default.
- Hook failures follow explicit fail-open/fail-closed policy.
- Hook outputs are schema-validated before mutating core state.
- Tests cover observation, mutation, blocking, disabled plugins, policy denial, and timeout behavior.

## Non-Goals

- Do not add plugin management UI.
- Do not expose arbitrary plugin UI events to remote clients yet; that is Phase 10.
- Do not add PyO3.
- Do not make process plugins sandboxed.
- Do not wire every hook type if central seams are not ready; prioritize correctness over breadth.

## Risks and Mitigations

### Risk: Hooks destabilize core agent execution

Mitigation: start with observation/post hooks, strict timeouts, and fail-open defaults. Add mutating hooks only behind policy.

### Risk: Process plugins become invisible interceptors

Mitigation: deny process lifecycle hooks by default. Require explicit trust/policy to enable.

### Risk: Provider/auth hooks leak secrets

Mitigation: defer until late in the phase and log only metadata. Require explicit secret permission declarations.

## Handoff Notes for Phase 10

Lifecycle hooks may produce UI effects. Phase 10 should route those effects through frontend-neutral core/TUI protocol events so all frontends can render or degrade them consistently.
