# Phase 12 Plan: Plugin Security and Policy Hardening

## Objective

Turn the plugin runtime/capability model into a policy-enforced trust boundary. The existing system already distinguishes builtin, process, and WASM runtimes and has conservative lifecycle hook defaults. This phase should centralize and harden capability checks, permission grants, runtime trust classes, lifecycle hook gates, output surface enforcement, and diagnostics.

The goal is defense in depth without making normal local development painful.

## Threat Model

Plugins can affect Codegg through several surfaces:

- slash commands;
- lifecycle hooks;
- UI effects;
- process execution;
- WASM execution;
- environment variables;
- filesystem access;
- model messages/context transforms;
- provider params/headers/auth;
- tool pre/post hooks;
- event subscriptions.

The most sensitive surfaces are:

- secrets and auth headers;
- model-visible messages;
- tool interception or mutation;
- shell environment mutation;
- filesystem writes;
- process lifecycle hooks;
- provider routing/auth hooks;
- plugin install/remove operations.

## Trust Classes

Formalize and enforce these trust classes:

### `Builtin`

First-party Rust code compiled into Codegg. Fully trusted but still represented through capabilities for observability and disable/enable behavior.

### `SandboxedWasm`

WASM module executed through Wasmtime with memory, fuel, timeout, module-size, and host-call limits. Treated as the preferred external runtime for lifecycle hooks.

### `LocalProcess`

Local executable process launched by Codegg. Not sandboxed. Useful for user-owned commands and scripts. Denied from lifecycle interception by default.

### `TrustedLocal`

Reserved for future embedded/PyO3/native extension style. Should not be used by default in this phase.

## Policy Modules

### `src/plugin/policy.rs`

Expand the current lifecycle policy into a broader plugin policy model:

```rust
pub struct PluginPolicy {
    pub lifecycle: PluginLifecyclePolicy,
    pub ui: PluginUiPolicy,
    pub permissions: PluginPermissionPolicy,
    pub install: PluginInstallPolicy,
    pub runtime: PluginRuntimePolicy,
}
```

Keep defaults conservative but usable:

- builtin commands/hooks allowed according to declared capabilities;
- WASM commands allowed;
- WASM lifecycle observation allowed where configured;
- process commands allowed when explicitly invoked by the user;
- process lifecycle hooks denied by default;
- undeclared capabilities denied;
- unknown output surfaces denied/degraded;
- secrets denied unless explicitly declared and granted.

### `src/plugin/permission.rs`

Add centralized permission evaluation if `policy.rs` becomes too large.

Recommended checks:

```rust
pub fn check_invocation_allowed(plugin: &PluginInfo, invocation: &PluginInvocation, policy: &PluginPolicy) -> PolicyDecision;
pub fn check_ui_effect_allowed(plugin: &PluginInfo, effect: &UiEffect, policy: &PluginPolicy) -> PolicyDecision;
pub fn check_lifecycle_hook_allowed(plugin: &PluginInfo, hook_type: HookType, policy: &PluginPolicy) -> PolicyDecision;
pub fn check_secret_access_allowed(plugin: &PluginInfo, secret_name: &str, policy: &PluginPolicy) -> PolicyDecision;
```

`PolicyDecision` should include allow/deny/degrade and an explanatory reason.

## Capability Enforcement

A plugin should only perform actions it declared.

Enforce declarations at these points:

1. `PluginService::invoke_command()` checks command capability exists.
2. `PluginService::dispatch_hook()` checks hook capability exists and runtime/trust is allowed.
3. `App::apply_plugin_ui_effect()` checks effect surface is declared/allowed for the source plugin when source metadata is available.
4. lifecycle helper methods check runtime/trust/category policy before dispatch.
5. plugin management commands show capability mismatch diagnostics.

## UI Effect Policy

Define output surfaces:

- chat;
- toast;
- dialog;
- panel;
- status item.

Rules:

- `EmitChat` requires `chat` output surface or command capability permitting chat.
- `ShowToast` requires toast or general command feedback permission.
- `OpenDialog` requires dialog output surface.
- `OpenPanel`/`UpdatePanel`/`ClosePanel` require panel capability or panel output surface.
- status item effects require status widget capability.
- IDs must be namespaced by plugin id for durable surfaces.
- plugin effects cannot displace permission/question/security dialogs.
- unsupported client surfaces degrade deterministically.

Add policy logging for denied/degraded UI effects.

## Lifecycle Hook Policy

Refine current defaults:

- observation hooks: builtin and WASM allowed when plugin system enabled;
- process observation hooks: denied by default unless `allow_process_lifecycle_hooks` true;
- mutating hooks: disabled by default; if enabled, builtin/WASM only by default;
- blocking hooks: disabled by default; if enabled, builtin/WASM only by default;
- auth/provider hooks: require explicit high-trust setting and secret access gates.

Add a per-hook category table in docs and tests.

## Secrets and Environment

### Process runtime env

Process plugins should receive only explicitly configured env entries. Do not inherit all Codegg secrets into plugin processes by default.

Rules:

- `env = ["KEY=value"]` passes literal values;
- `env = ["KEY"]` may pass through from current environment only if policy allows pass-through;
- secret names should be resolved only through explicit secret permission declarations;
- logs must include env keys only, not values.

### Shell env hook

Shell env hooks should be treated as mutating hooks and denied by default. If enabled, outputs should be validated:

- key names must be sane;
- values should not be logged;
- remove list should not include critical variables unless allowed;
- process runtime lifecycle hooks remain denied unless explicitly allowed.

## Filesystem and Install Safety

Policy should distinguish:

- no filesystem;
- project read;
- project write;
- plugin install dir read/write;
- full filesystem, discouraged.

Immediate hardening targets:

- plugin install rejects path traversal;
- remove refuses paths outside plugin install dir;
- process cwd defaults to project dir or plugin dir by explicit policy;
- WASM module path must live under plugin install dir unless explicitly allowed;
- doctor reports manifests whose runtime artifact is outside expected location.

## Runtime Enforcement

### ProcessRuntime

Ensure:

- no shell by default;
- timeout required/defaulted;
- stdout/stderr caps enforced;
- cwd controlled;
- env allowlist/passthrough policy enforced;
- nonzero exits preserve diagnostics;
- no lifecycle hooks unless policy allows.

### WasmRuntime

Ensure:

- module size limit;
- fuel budget corrected;
- timeout;
- memory limit, or explicit note if memory max is not actively enforced yet;
- output cap;
- no host calls beyond minimal ABI;
- module path constrained;
- feature-disabled behavior is explicit.

### BuiltinRuntime

Ensure:

- hook-only unless command registry is implemented;
- unknown capabilities rejected;
- unknown hook strings rejected;
- builtins declare capabilities for observability.

## Diagnostics and Audit Logging

Add structured logs for policy decisions:

- plugin id;
- runtime kind;
- trust class;
- capability type;
- hook type;
- decision: allow/deny/degrade;
- reason;
- duration;
- error/timeout.

Do not log secrets, full auth headers, full provider messages, or raw env values by default.

Add `/plugin-doctor` integration from Phase 11 if available:

- show policy-denied capabilities;
- show missing permissions;
- show high-risk grants;
- show runtime restrictions.

## Tests

Add policy unit tests:

- undeclared command denied;
- declared command allowed;
- undeclared dialog effect denied;
- status effect requires status widget capability;
- panel update cannot target another plugin’s id;
- process lifecycle hook denied by default;
- process lifecycle hook allowed only when policy flips;
- mutating hooks denied by default;
- blocking hooks denied by default;
- auth/provider hooks require explicit high-trust policy;
- secret access denied without declaration;
- env passthrough denied by default;
- remove refuses outside install dir;
- WASM module outside plugin dir denied or warned.

Add integration tests:

- plugin command with undeclared output surface has effect degraded/denied;
- lifecycle hook response with unauthorized UI effect is filtered;
- process command receives only allowed env keys;
- policy denial surfaces a useful diagnostic in `/plugin-doctor` or equivalent.

## Documentation Updates

Update:

- `docs/PLUGINS.md` with policy tables;
- `architecture/plugin.md` with trust model;
- `.opencode/skills/plugin/SKILL.md` with security invariants;
- example manifests with minimal permissions.

Include a direct statement:

> Process plugins are local executable code. They are not sandboxed. They are suitable for explicit user-invoked local commands, not silent lifecycle interception by default.

## Acceptance Criteria

- Capability checks are centralized and testable.
- Runtime/trust policy is enforced for commands, hooks, UI effects, secrets, env, and install/remove operations.
- Process lifecycle hooks remain denied by default.
- UI effects are filtered by declared output surfaces.
- Sensitive values are not logged.
- Doctor/diagnostic surfaces explain policy denials.
- Tests cover policy allow/deny/degrade behavior across runtimes and capability classes.
