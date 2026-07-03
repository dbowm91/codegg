# Agent Customization Milestone 6: Safe Overlays and Rich Permissions

## Goal

Make custom agent overrides safe and expressive. Users should be able to modify built-ins without accidentally erasing prompts or security-critical deny rules, and agents should support structured permission rules for bash and paths.

## Overlay semantics

File-based overlays should merge by default when they use the same name as an existing agent.

Rules:

```text
new name -> create new custom agent
same name + no replace flag -> merge into existing resolved source stack
same name + merge = true -> merge into existing
same name + replace = true -> replace existing definition, then apply later layers
same name + disable = true -> disable final agent unless later layer re-enables
```

Built-in replacement should require explicit `replace = true` and should emit a warning-level diagnostic. Most user intent will be simple merge overrides.

Example safe built-in override:

```toml
schema_version = 1
name = "security-review"
model = "tier.frontier"
temperature = 0.05
```

This should keep the built-in prompt, role, mode, permissions, and runtime behavior.

## Merge rules

Recommended merge behavior:

- Scalar fields replace prior values when set.
- Missing fields leave prior values unchanged.
- Permission maps merge per tool.
- Prompt replacement requires `prompt` or `prompt_file`.
- `hidden`, `disable`, and `mode` replace prior values when explicitly set.
- `options` merge by key.
- `replace = true` resets the accumulated spec before applying the current file.

Diagnostics should explain replacement versus merge:

```text
info: .codegg/agents/security-review.toml merged into built-in security-review
warning: .codegg/agents/security-review.toml replaces built-in security-review because replace = true
```

## Rich permission schema

Keep simple compatibility:

```toml
[permission]
read = "allow"
edit = "deny"
bash = "ask"
```

Add structured rules:

```toml
[permission.bash]
action = "ask"
allow_patterns = ["git diff*", "git status*", "cargo test*", "cargo audit*"]
deny_patterns = ["curl*", "wget*", "git push*", "rm *"]

[permission.paths]
allow = ["src/**", "crates/**"]
deny = [".git/**", "target/**", "**/.env"]
```

If current config types cannot represent this cleanly, add an agent-specific permission spec and convert it to existing `PermissionRule`/`ToolRule` structures during resolution.

## Permission conversion

Final runtime permissions should be converted into the existing permission checker structures so tool calls use one enforcement path.

Conversion requirements:

- Simple `permission.tool = "allow"` maps to a tool-level rule.
- `permission.bash.action` sets the default bash action.
- `permission.bash.allow_patterns` becomes allow-pattern rules.
- `permission.bash.deny_patterns` becomes deny-pattern rules.
- Path allow/deny patterns become path-scoped agent rules if the existing checker supports them.
- Invalid permission actions produce diagnostics.

## Safety envelope

Agent permissions must not silently exceed session/runtime safety constraints.

Effective permission decision should be the most restrictive result across:

```text
resolved agent permission
session permission override
global config permission
active sandbox/approval policy
hard safety policy
```

Do not allow a custom agent file to silently escalate from `ask` to `allow` for dangerous operations if the active session policy is stricter.

## Tests

Add tests for overlay behavior:

- Project file with only `model` changes built-in model while preserving prompt and permissions.
- Project file with `replace = true` replaces built-in and emits diagnostic.
- Global override is superseded by project override.
- Config override supersedes project override.
- `disable = true` hides/disables an agent.
- Missing fields do not erase prior values.

Add tests for permissions:

- Simple permission strings still work.
- Structured bash allow/deny patterns convert into runtime rules.
- Deny pattern wins over broad allow where applicable.
- Path allow/deny rules are represented or diagnosed if unsupported.
- Custom agent cannot bypass session safety envelope.

## Acceptance criteria

- Overlays merge by default.
- Explicit replacement is supported and diagnosed.
- Agent disabling is supported.
- Structured bash/path permissions are supported or clearly diagnosed.
- Existing simple permission config remains backward compatible.
- Effective permissions are bounded by runtime/session safety constraints.

## Handoff notes

This is a security-sensitive milestone. Prefer conservative behavior where ambiguity exists. If a permission rule cannot be represented faithfully, emit a diagnostic and choose the safer result rather than silently allowing more access.
