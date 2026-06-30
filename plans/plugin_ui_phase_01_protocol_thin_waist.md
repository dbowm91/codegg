# Phase 1 Plan: Protocol Thin Waist for Plugin UI and Invocation

## Objective

Introduce frontend-neutral plugin and UI protocol DTOs in `crates/codegg-protocol` so later plugin runtimes, TUI rendering, remote clients, and process/WASM command paths all share one stable envelope.

This phase is intentionally protocol-only. It should not execute plugins, modify command dispatch, or add ratatui rendering. It establishes the data model that later phases consume.

## Why This Phase Comes First

The repo already has `codegg-protocol` as the shared transport crate for core, DTOs, frames, and TUI protocol. Plugin UI must live at this level because Codegg is moving toward multiple frontends. If plugin UI types are introduced in `src/tui` or `src/plugin/tui.rs`, plugin authors will inherit ratatui-specific concepts and the future GUI/web/mobile clients will need translation shims.

The thin waist should be:

- serializable;
- dependency-light;
- stable enough for examples and SDKs;
- independent of ratatui/crossterm/root `App` internals;
- versioned enough to evolve.

## Files to Add

### `crates/codegg-protocol/src/ui.rs`

Add frontend-neutral UI DTOs.

Recommended initial types:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiNode {
    Text(TextNode),
    Markdown(MarkdownNode),
    Code(CodeNode),
    Table(TableNode),
    KeyValue(KeyValueNode),
    Progress(ProgressNode),
    Container(ContainerNode),
    Empty,
    Unsupported { kind: String, data: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextNode {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarkdownNode {
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeNode {
    pub language: Option<String>,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableNode {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyValueNode {
    pub entries: Vec<KeyValueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyValueEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressNode {
    pub label: Option<String>,
    pub current: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerNode {
    pub title: Option<String>,
    pub children: Vec<UiNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEffect {
    EmitChat { block: ChatBlock },
    ShowToast { toast: ToastSpec },
    OpenDialog { dialog: DialogSpec },
    CloseDialog { id: String },
    OpenPanel { panel: PanelSpec },
    UpdatePanel { id: String, body: UiNode },
    ClosePanel { id: String },
    AddStatusItem { item: StatusItemSpec },
    UpdateStatusItem { id: String, body: UiNode },
    RemoveStatusItem { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatBlock {
    pub format: ChatFormat,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatFormat {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToastSpec {
    pub level: ToastLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DialogSpec {
    pub id: String,
    pub title: String,
    pub body: UiNode,
    pub modal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
    pub placement: PanelPlacement,
    pub body: UiNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    Left,
    Right,
    Bottom,
    Main,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusItemSpec {
    pub id: String,
    pub label: Option<String>,
    pub placement: StatusPlacement,
    pub body: UiNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StatusPlacement {
    Left,
    Right,
}
```

Do not overbuild forms/diffs/trees in this phase. Add `Unsupported` for forward-compatible fallback.

### `crates/codegg-protocol/src/plugin.rs`

Add plugin protocol DTOs.

Recommended initial types:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ui::UiEffect;

pub const PLUGIN_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifestDto {
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub runtime: PluginRuntimeSpec,
    pub capabilities: Vec<PluginCapability>,
    pub permissions: PluginPermissionSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginRuntimeSpec {
    Builtin { handler: String },
    Process { command: String, args: Vec<String>, timeout_ms: Option<u64> },
    Wasm { module: String, timeout_ms: Option<u64>, memory_max_mb: Option<u64>, fuel_per_call: Option<u64> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCapability {
    Command(PluginCommandSpec),
    Hook(PluginHookSpec),
    Panel(PluginPanelContribution),
    StatusWidget(PluginStatusContribution),
    EventSubscription(PluginEventSubscription),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginCommandSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub handler: Option<String>,
    pub output: Vec<PluginOutputSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginHookSpec {
    pub hook_type: String,
    pub priority: i32,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginPanelContribution {
    pub id: String,
    pub title: String,
    pub placement: String,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginStatusContribution {
    pub id: String,
    pub label: Option<String>,
    pub placement: String,
    pub refresh_ms: Option<u64>,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginEventSubscription {
    pub event_type: String,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginOutputSurface {
    Chat,
    Toast,
    Dialog,
    Panel,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginPermissionSet {
    pub network: bool,
    pub filesystem: FilesystemPermission,
    pub env: Vec<String>,
    pub secrets: Vec<String>,
    pub session_messages: bool,
    pub tool_interception: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemPermission {
    #[default]
    None,
    ProjectRead,
    ProjectWrite,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginInvocation {
    pub protocol_version: u32,
    pub invocation_id: String,
    pub plugin_id: String,
    pub capability: PluginCapabilityInvocation,
    pub args: Vec<String>,
    pub input: serde_json::Value,
    pub context: PluginContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCapabilityInvocation {
    Command { name: String },
    Hook { hook_type: String },
    Panel { id: String },
    StatusWidget { id: String },
    Event { event_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginContext {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub project_dir: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub frontend_capabilities: Vec<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginResponse {
    pub ok: bool,
    pub effects: Vec<UiEffect>,
    pub data: serde_json::Value,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginDiagnostic {
    pub level: PluginDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticLevel {
    Debug,
    Info,
    Warning,
    Error,
}
```

Keep hook types as strings in the protocol DTO initially. The root crate can map them to richer enums later. This avoids forcing old `src/plugin/hooks.rs` directly into the shared protocol.

### `crates/codegg-protocol/src/lib.rs`

Export the new modules:

```rust
pub mod plugin;
pub mod ui;
```

## Files to Modify

### `crates/codegg-protocol/Cargo.toml`

Normally no new dependency should be needed beyond existing serde/serde_json. If serde_json is not currently present in this crate, add it only here.

### Tests

Add unit tests in `ui.rs` and `plugin.rs`, or under a new protocol test module. Tests should cover:

- serde round trip for `UiNode::Table`;
- serde round trip for `UiEffect::OpenDialog`;
- serde round trip for `PluginInvocation` command;
- serde round trip for `PluginResponse` carrying a chat effect and dialog effect;
- default filesystem permission is `None`;
- unknown/future UI content can degrade through `Unsupported`.

## Implementation Steps

1. Add `ui.rs` with minimal node/effect structs.
2. Add `plugin.rs` with manifest, runtime, capability, invocation, response, permission, and diagnostic DTOs.
3. Export modules from `lib.rs`.
4. Add serde round-trip tests.
5. Run `cargo test -p codegg-protocol`.
6. Run root-level `cargo test` or the repo’s standard fast test command if available.
7. Update docs only if compilation requires a brief note. Full docs come later.

## Compatibility Rules

- Do not modify existing `TuiMessage` or `CoreEvent` in this phase unless absolutely necessary.
- Do not reference ratatui or crossterm.
- Do not reference `src/plugin` types.
- Do not add plugin execution.
- Do not remove old plugin code.
- Do not make plugin protocol types depend on root crate types.

## Acceptance Criteria

- `codegg-protocol` compiles independently.
- New DTOs are serializable/deserializable.
- Unit tests prove stable JSON shape for the first command/dialog/table cases.
- Existing protocol tests still pass.
- No frontend or root crate dependency is introduced into `codegg-protocol`.

## Risks and Mitigations

### Risk: Overbuilding the UI schema

Mitigation: limit Phase 1 to simple informational output. Defer forms, trees, diffs, streaming logs, and command palette contribution payloads.

### Risk: Coupling protocol to current TUI enum names

Mitigation: use generic semantic names such as `DialogSpec`, `PanelSpec`, `StatusItemSpec`, and `UiNode` rather than `DialogType` or ratatui widget names.

### Risk: Locking in hook type enum too early

Mitigation: keep hook type as string in protocol. Root code can validate and map to internal enums.

## Handoff Notes for Phase 2

Phase 2 should consume `codegg_protocol::ui::UiNode` and `UiEffect` from the TUI crate/root crate. It should not change the protocol types unless Phase 1 missed a critical field. Any missing renderer behavior should degrade gracefully rather than expanding the schema prematurely.
