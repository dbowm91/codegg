# Phase 15 Plan: Multi-Frontend Plugin UI Readiness

## Objective

Make plugin UI, management, lifecycle effects, and durable plugin surfaces robust across embedded TUI, remote TUI, daemon/socket clients, and future GUI/web/mobile frontends.

By this point, Codegg has protocol-level `UiEffect`, `UiEffectEnvelope`, remote `TuiMessage::PluginUiEffect`, durable panel/status snapshot metadata, and client capability flags. This phase turns that foundation into a stable multi-frontend contract.

## Frontend Classes

Support these client classes explicitly:

### Embedded TUI

Runs in-process. Can render most ratatui-backed plugin UI directly.

### Remote TUI

Receives protocol events/snapshots over socket/stdio. Should receive session-scoped plugin UI effects and durable plugin surface metadata.

### CLI/Automation

May not support dialogs/panels/status widgets. Should degrade to text/chat/log output deterministically.

### GUI/Web/Mobile Future Clients

Should consume the same `UiNode`/`UiEffect` protocol without depending on ratatui naming or TUI-only dialog enums.

## Protocol Capability Negotiation

Extend or stabilize capability negotiation around plugin UI.

Recommended capabilities:

```rust
pub struct ClientCapabilities {
    pub visual_notifications: bool,
    pub desktop_notifications: bool,
    pub audio: bool,
    pub tts: bool,
    pub multi_session_view: bool,
    pub plugin_ui_dialogs: bool,
    pub plugin_ui_panels: bool,
    pub plugin_ui_status_items: bool,
    pub plugin_ui_tables: bool,
    pub plugin_ui_markdown: bool,
    pub plugin_ui_code: bool,
    pub plugin_ui_progress: bool,
}
```

If `PluginUiCapabilities` already exists separately, map it cleanly into `ClientCapabilities` rather than duplicating policy logic.

## Event Transport Rules

### Session-scoped effects

Plugin effects that belong to a session should use core event transport:

```text
PluginRuntime -> PluginResponse.effects -> AppEvent/CoreEvent::PluginUiEffect -> subscribed clients
```

### Local-only effects

Effects produced by purely local TUI commands may stay local unless they should be visible to other clients.

### Durable surfaces

Panels and status items are durable enough to include in snapshots. Dialogs and toasts are transient unless event replay already reconstructs them safely.

### Ordering

Effects from one plugin response must preserve order. Use existing event sequence ordering rather than inventing a separate sequence system.

## Snapshot Requirements

Remote snapshots should include enough metadata for durable plugin surfaces:

- panel id;
- title;
- placement;
- source plugin id if available;
- body summary or body payload if size-safe;
- status item id;
- label;
- placement;
- value/body summary;
- version/update sequence if available.

Current snapshots include panel/status metadata. This phase should decide whether body payloads are needed for reconnect fidelity.

Recommendation:

- include lightweight body payloads for durable panels/status items if size limits allow;
- otherwise include metadata and rely on replay/resync to fetch body.

## Degradation Matrix

Define and test deterministic degradation for each frontend capability set.

| Effect | Full UI client | Text-only client | Unsupported/automation |
| --- | --- | --- | --- |
| `EmitChat` | visible UI/chat surface | stdout/log text | log text |
| `ShowToast` | toast | prefixed text line | optional log |
| `OpenDialog` | modal/dialog | title + body text | log/report |
| `OpenPanel` | panel | heading + body text | omit or log summary |
| `AddStatusItem` | status bar | optional line | omit |
| `UpdatePanel` | update existing panel | text update | omit |
| `Close*` | close surface | no-op | no-op |

Add helpers so degradation is not duplicated across clients.

## Size and Backpressure Limits

Plugin UI can be abused as an output channel. Add or enforce limits:

- max effect count per response;
- max serialized effect bytes;
- max node depth;
- max table rows/columns;
- max string length per node;
- max durable panel/status item count per plugin;
- max open plugin dialogs globally;
- event log retention for plugin UI effects.

Policy should deny or truncate with diagnostics rather than panic.

## Source Attribution and Ownership

Every plugin UI effect should carry source metadata when crossing process/core/frontend boundaries:

- plugin id;
- invocation id;
- session id;
- runtime kind if useful;
- trust class optionally in diagnostics/management views.

TUI already validates durable surface ownership. Extend this to remote/core paths:

- source plugin id must match durable surface id namespace;
- cross-plugin updates are rejected;
- missing source id for durable effects is rejected or namespaced under a safe synthetic source.

## Multi-Client Behavior

When multiple frontends are connected:

- session-scoped plugin effects should be sent to subscribers of that session;
- durable state changes should update snapshots for new/reconnected clients;
- local-only effects should not leak to unrelated sessions;
- automation clients should not block on unsupported UI;
- unsupported clients should receive degraded text or ignore safely.

Add tests for at least two clients with different capabilities if current core/client test harness permits.

## Files to Modify

### `crates/codegg-protocol/src/core.rs`

Ensure `CoreEvent::PluginUiEffect` or equivalent carries `UiEffectEnvelope` rather than ad hoc fields if possible.

### `crates/codegg-protocol/src/tui.rs`

Ensure remote `TuiMessage::PluginUiEffect` and snapshots are versioned and backward-compatible.

Add body payloads to durable panel/status snapshot only if size caps are implemented.

### `crates/codegg-protocol/src/ui.rs`

Add shared degradation and limit-validation helpers if not already present:

```rust
pub fn validate_ui_effect(effect: &UiEffect, limits: &UiLimits) -> Result<(), UiValidationError>;
pub fn degrade_effect(effect: &UiEffect, caps: &PluginUiCapabilities) -> Option<UiEffect>;
pub fn effect_summary(effect: &UiEffect) -> Option<String>;
```

### `src/core/daemon.rs`

Ensure plugin UI events are published through the same event log/replay path as other session events.

### `src/tui/app/mod.rs`

Ensure remote event handling:

- filters session id;
- validates source ownership;
- applies effect through one helper;
- degrades unsupported effects;
- does not displace critical modals;
- updates durable plugin state.

### `src/tui/runtime/app_events.rs`

Ensure app/core event bridge maps plugin UI effects bidirectionally where needed.

## Tests

Protocol tests:

- serde round trip for `UiEffectEnvelope`;
- old snapshot JSON without plugin fields still deserializes;
- snapshot with plugin durable surfaces serializes;
- effect validation rejects over-limit payloads;
- degradation matrix for text-only capabilities.

Core tests:

- lifecycle hook UI effect emits core event;
- event order preserved for multiple effects;
- client with unsupported surfaces receives degraded or ignored output;
- session mismatch prevents delivery/application.

TUI tests:

- remote dialog effect opens/stores dialog;
- remote panel effect stores durable panel;
- remote status effect stores durable status item;
- remote `EmitChat` visible but not model-context-visible;
- cross-plugin panel update rejected;
- permission dialog not displaced;
- snapshot includes durable plugin surface metadata/body as designed.

Multi-client tests if available:

- two clients with different capabilities receive appropriate forms;
- reconnect snapshot includes durable surfaces;
- transient toast not replayed unless explicitly retained.

## Documentation Updates

Update:

- `architecture/plugin.md` with multi-frontend UI flow;
- `architecture/tui.md` with remote plugin UI handling;
- `docs/PLUGINS.md` with frontend compatibility notes;
- protocol docs if present.

Document:

- capability negotiation;
- degradation rules;
- transient vs durable surfaces;
- snapshot/replay behavior;
- size limits;
- session scoping;
- ownership/namespacing.

## Acceptance Criteria

- Plugin UI event transport works over embedded and remote TUI paths.
- Client capability negotiation controls rendering/degradation.
- Durable panel/status plugin surfaces survive snapshot/resync as designed.
- Unsupported clients degrade or ignore safely.
- Source ownership is enforced across remote/core paths.
- UI payload limits prevent oversized plugin output from destabilizing clients.
- Tests cover protocol, TUI, core event, session filtering, ownership, degradation, and snapshot behavior.
