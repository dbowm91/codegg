# codegg-protocol

Wire and domain protocol contracts for CodeGG daemon/frontend
communication.

This crate contains only data contracts: request/response/event DTOs for
core, provider, projection, TUI, plugin, LSP, runtime-asset, interactive
process, and UI surfaces. It owns no runtime services, no I/O, no daemon
or scheduler authority, and no frontend rendering.

The package is intentionally CodeGG-specific, but it is independently
consumable: third-party and front-end clients depend only on this crate
(plus `serde` / `serde_json` / `thiserror`) without depending on the
root `codegg` application.

## Compatibility

- `core::PROTOCOL_VERSION` is the core contract version (currently 2).
  Per-surface versions also exist (`REMOTE_TUI_PROTOCOL_VERSION`,
  `PLUGIN_PROTOCOL_VERSION`,
  `INTERACTIVE_PROCESS_PROTOCOL_VERSION`, projection
  `PROJECTION_PROTOCOL_VERSION`).
- Additive variants and optional fields are minor: old clients that
  ignore unknown variants remain forward-compatible.
- Renaming, removing, or retyping an existing wire field, or changing
  the meaning of an existing variant, requires a version bump.
- Payload and collection bounds are part of the contract (see
  `projection::limits`); producers must respect them so consumers can
  rely on bounded decoding.
