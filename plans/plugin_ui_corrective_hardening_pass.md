# Plugin UI Runtime Corrective and Hardening Plan

## Objective

Close the remaining correctness gaps in the plugin UI/runtime integration before expanding into plugin management UX, SDKs, or broader lifecycle-hook coverage.

The current architecture is mostly aligned with the roadmap: runtime abstraction exists, process execution has moved into `ProcessRuntime`, WASM and builtin runtimes are implemented, lifecycle policy exists, and remote plugin UI events are transportable. This pass should focus on hardening the parts that can become trust-boundary or user-visible bugs.

## Scope

This plan targets four concrete issues found in the latest review:

1. WASM fuel accounting returns consumed fuel instead of unused fuel.
2. Builtin runtime silently maps unsupported/non-hook capabilities to `HookType::Auth`.
3. `EmitChat` effects are accepted but not visibly rendered in the TUI.
4. Plugin registry enabled filtering still uses `try_read()` and can produce transient false negatives under contention.

Do not expand feature scope in this pass. The goal is correctness and deterministic behavior.

## Non-Goals

- Do not add plugin install/enable/disable slash commands.
- Do not add plugin marketplace or remote install behavior.
- Do not add PyO3.
- Do not add new lifecycle hook locations beyond test coverage needed for this pass.
- Do not introduce a new WASM ABI.
- Do not refactor the whole TUI message model.

## Phase A: Correct WASM Fuel Accounting

### Problem

`src/plugin/runtime/wasm.rs` reserves `fuel_per_call` from `WasmModuleCache`, runs a WASM invocation, then computes:

```rust
let remaining = store.get_fuel().unwrap_or(0);
let consumed = fuel.saturating_sub(remaining);
if consumed > 0 {
    cache.return_fuel(plugin_id, consumed);
}
```

Since `reserve_fuel()` already subtracts the full reserved amount from the plugin budget, returning `consumed` adds back the used fuel. The correct value to return is the unused remainder of the reservation, normally `remaining`, capped by the originally reserved amount.

The legacy fallback path has the same pattern.

### Implementation Steps

1. In `src/plugin/runtime/wasm.rs`, update both modern and legacy paths:

```rust
let remaining = store.get_fuel().unwrap_or(0).min(fuel);
if remaining > 0 {
    cache.return_fuel(plugin_id, remaining);
}
```

2. Remove or rename variables that imply consumed fuel is being returned.
3. Add a helper to avoid duplicating the accounting logic:

```rust
fn return_unused_fuel(cache: &WasmModuleCache, plugin_id: &str, reserved: u64, remaining: u64) {
    let unused = remaining.min(reserved);
    if unused > 0 {
        cache.return_fuel(plugin_id, unused);
    }
}
```

4. Make sure the error path still returns the full reserved amount when invocation fails before reliable fuel consumption is known.
5. If an invocation traps after consuming fuel, decide explicitly whether to return all reserved fuel or only known remaining fuel. Prefer returning all reserved fuel only when the trap happens before store execution begins. Once store execution begins, return only `remaining` if available.

### Tests

Add unit tests in `src/plugin/runtime/wasm_cache.rs` and feature-gated runtime tests if possible.

Minimum cache-level tests:

- reserve 1000 from max budget, return 700 unused, remaining budget is `MAX - 300`.
- reserve 1000, return 0, remaining budget is `MAX - 1000`.
- returning more than reserved through the helper does not exceed expected budget for that reservation.

If direct helper tests live in `wasm.rs`, gate them appropriately.

Feature-gated test target, if practical:

- a tiny WASM fixture consumes some fuel and returns successfully; budget decreases by more than zero and less than/equal to reserved amount.

### Acceptance Criteria

- Modern ABI path returns unused fuel, not consumed fuel.
- Legacy ABI path returns unused fuel, not consumed fuel.
- Error paths do not double-return fuel.
- Tests fail under the old consumed-fuel behavior and pass under the corrected behavior.
- Comments accurately describe fuel semantics.

## Phase B: Make Builtin Runtime Capability Handling Strict

### Problem

`src/plugin/runtime/builtin.rs` currently permits `PluginCapabilityInvocation::Command` by mapping it to the string `command`, then parses the hook type with fallback:

```rust
let hook_type = HookType::parse(hook_type_str).unwrap_or(HookType::Auth);
```

This means unsupported or unknown builtin invocations can silently become `Auth` hook invocations. That is unsafe and misleading at a runtime boundary.

### Implementation Steps

1. Change `invocation_to_hook_context()` so it accepts only `PluginCapabilityInvocation::Hook` for current builtin hook handlers.
2. If command-capable builtins are desired later, add a separate command handler registry. Do not route commands through `HookContext`.
3. Replace `unwrap_or(HookType::Auth)` with a hard error:

```rust
let hook_type = HookType::parse(hook_type_str).ok_or_else(|| {
    RuntimeError::Unsupported(format!("unsupported builtin hook type: {hook_type_str}"))
})?;
```

4. Ensure `BuiltinRuntime::invoke()` returns `RuntimeError::Unsupported` for:
   - command invocations;
   - panel/status/event invocations if present;
   - unknown hook strings;
   - plugin ids without `builtin:` prefix.
5. Update `PluginService::invoke_command()` builtin branch. If a builtin plugin declares a command but no command runtime handler exists, return a clear unsupported runtime error rather than a success placeholder.
6. Check `src/plugin/builtin/mod.rs` and builtin manifests. Do not declare command capabilities unless a real builtin command handler exists.

### Tests

Add/adjust tests in `src/plugin/runtime/builtin.rs`:

- known builtin hook dispatch succeeds.
- unknown builtin handler fails.
- non-builtin plugin id fails.
- command invocation fails with `RuntimeError::Unsupported`.
- unknown hook type fails with `RuntimeError::Unsupported`.
- no unsupported invocation falls back to `HookType::Auth`.

Add service-level test if current harness permits:

- builtin command declaration without command handler returns `PluginError::Runtime`, not success.

### Acceptance Criteria

- Builtin runtime no longer silently treats unsupported invocation types as auth hooks.
- Unknown hook strings are rejected.
- Builtin command dispatch is either explicitly implemented or explicitly unsupported.
- Tests protect against fallback-to-auth regression.

## Phase C: Render `EmitChat` Effects in the TUI

### Problem

`ProcessRuntime` converts plain stdout into `PluginResponse { effects: [UiEffect::EmitChat { ... }] }`. `PluginUiState::apply_effect()` returns `ChatRequested` for `EmitChat`, but current callers mostly log that chat handling is deferred. This can make stdout-only `/quota`-style commands invisible to users.

### Implementation Steps

1. Add an `App` helper that converts `ChatBlock` into visible message output:

```rust
fn apply_plugin_chat_block(&mut self, block: ChatBlock, source_plugin_id: Option<&str>) {
    // Append to the chat transcript or show_short_or_info, depending on existing message model constraints.
}
```

2. Prefer appending to the visible chat/message stream as a plugin/system-style message if the message model supports it. If there is no plugin/system role, use a user-visible assistant/info message with a plugin prefix:

```text
[plugin:<id>]
<content>
```

3. If appending to transcript risks polluting model context, use an explicit non-model-visible UI message surface if one exists. The key requirement is visibility to the user without silently adding tool/plugin output into future model context unless intended.
4. Update `App::apply_plugin_ui_effect()` so `UiEffect::EmitChat { block }` is handled directly, not delegated to `PluginUiState` as `ChatRequested`.
5. Update all call sites in:
   - `src/tui/commands/plugins.rs`
   - remote event handling in `src/tui/app/mod.rs`
   - any core event handling path
   so `ChatRequested` is no longer treated as normal for `EmitChat` after the helper exists.
6. Keep `PluginUiApplyResult::ChatRequested` only if needed for a lower-level state-only path; otherwise remove or stop producing it for normal app-level effect application.
7. Ensure `ChatFormat::Plain` and `ChatFormat::Markdown` both render safely. Do not execute markdown links or raw terminal escape sequences.

### Tests

Add TUI tests:

- process command stdout text becomes visible in messages or an info surface.
- structured `PluginResponse` with `EmitChat` becomes visible.
- remote `PluginUiEffect::EmitChat` becomes visible when session matches.
- remote `EmitChat` is ignored when session id does not match.
- `ChatFormat::Plain` and `ChatFormat::Markdown` both render.
- plugin chat output does not displace permission/question/security dialogs.
- if plugin chat output is not model-context-visible, assert that provider context construction excludes it.
- if plugin chat output is model-context-visible by design, document and test that behavior.

### Acceptance Criteria

- A stdout-only process command produces visible output.
- `EmitChat` no longer disappears into debug logs.
- Remote plugin chat effects are visible when session-matched.
- Output visibility semantics relative to model context are explicit and tested.

## Phase D: Make Registry Enabled Filtering Deterministic

### Problem

`PluginRegistry` capability queries still use helpers based on `try_read()` to check whether a plugin is enabled while holding other collection locks. The latest behavior defaults to disabled on contention, which is safer than defaulting enabled, but it can still make enabled capabilities transiently disappear.

### Preferred Fix

Avoid `try_read()` entirely in capability queries. Snapshot enabled plugin ids first, then filter capability vectors against that snapshot.

Example:

```rust
async fn enabled_plugin_ids(&self) -> HashSet<String> {
    self.plugins
        .read()
        .await
        .iter()
        .filter_map(|(id, info)| info.enabled.then(|| id.clone()))
        .collect()
}
```

Then:

```rust
pub async fn commands(&self) -> Vec<PluginCommandRegistration> {
    let enabled = self.enabled_plugin_ids().await;
    self.commands
        .read()
        .await
        .iter()
        .filter(|c| enabled.contains(&c.plugin_id))
        .cloned()
        .collect()
}
```

Apply the same pattern to:

- `hooks_for()`
- `command()`
- `commands()`
- `panels()`
- `status_widgets()`
- `event_subscribers()`
- duplicate checks inside `set_enabled()` if they still rely on `is_enabled_in()`.

### Alternative Fix

If query performance becomes a concern, maintain enabled capability indexes or use a single internal lock struct:

```rust
struct PluginRegistryInner {
    plugins: HashMap<String, PluginInfo>,
    hooks: Vec<...>,
    commands: Vec<...>,
    ...
}
```

Then all queries operate under one read lock. This is simpler semantically but a larger refactor. Prefer the enabled-id snapshot for this corrective pass.

### Tests

Add deterministic tests:

- enabled plugin command is returned.
- disabled plugin command is not returned.
- no capability query uses contention-sensitive fallback.
- re-enable after disable returns capabilities.
- duplicate command re-enable behavior remains correct.

Add a concurrency-style regression test if feasible:

- spawn repeated command queries while toggling an unrelated plugin; enabled plugin capabilities should not randomly disappear except according to actual enabled state.

This test does not need to be probabilistic if the helper no longer uses `try_read()`.

### Acceptance Criteria

- `try_read()` is not used for enabled filtering in normal capability queries.
- Capability visibility depends only on actual enabled state, not lock contention.
- Existing duplicate and enable/disable semantics remain intact.
- Tests cover enabled, disabled, re-enabled, and duplicate cases.

## Phase E: Validation and Documentation

### Validation Commands

Run the repo’s standard validation suite. At minimum:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo test --features plugins
cargo check --features plugins
```

If all-features is too expensive or not supported, document the exact alternative that was run.

### Documentation Updates

Update docs only where behavior changes:

- `architecture/plugin.md`: clarify WASM fuel budget semantics.
- `docs/PLUGINS.md`: document that `EmitChat` renders visibly and whether it enters model context.
- `.opencode/skills/plugin/SKILL.md`: note builtin runtime strictness if it describes builtin command behavior.

### Acceptance Criteria

- Validation commands pass or failures are documented with concrete reasons.
- No docs describe consumed fuel being returned.
- No docs suggest builtin commands are supported unless they are actually implemented.
- Plugin stdout/`EmitChat` behavior is documented.

## Final Definition of Done

This corrective pass is complete when:

1. WASM fuel budgets decrease by consumed fuel, not unused fuel.
2. Builtin runtime rejects unsupported invocation types and unknown hook types.
3. Plain stdout process commands visibly render through `EmitChat` or a documented equivalent.
4. Registry capability filtering is deterministic and no longer relies on `try_read()` fallbacks.
5. Tests cover all four corrections.
6. The repo validates with normal cargo test/clippy commands, including `plugins` feature checks if available.
