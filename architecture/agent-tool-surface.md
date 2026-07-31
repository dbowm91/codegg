# Resolved agent tool surface

Each agent turn resolves one immutable `ResolvedToolSurface` in
`src/agent/tool_surface.rs`. The surface is built from the registered model
definitions after native plan/model exposure filtering and before provider
deferral. It contains canonical and wire names, backend kind, category,
schema, required/never-reduce markers, omissions, capabilities, and a stable
SHA-256 fingerprint.

Resolution is monotonic: registered definitions are narrowed by explicit
denies, model disables, plan mode, callable-backend availability, and an
optional parent capability ceiling. Roles and agent names are prompt metadata
only. Delegation is represented by `Capability::Delegate` and does not imply
filesystem mutation authority.

Provider/model aliases enter through `resolve_with_aliases`. The resolver
keeps both directions of the mapping so a provider wire call can be
normalized to the canonical name before permission and broker execution.
Native and MCP names remain distinct; ambiguous aliases are rejected.

Context palette reduction uses `ResolvedToolSurface::reduce`, which always
starts from the unreduced immutable surface. A failed or empty reduction can
therefore restore `definitions()` without reconstructing registry state.

The permission checker and tool broker remain the execution authorities. The
surface controls advertisement and records bounded omission and fingerprint
diagnostics; it does not duplicate permission prompts or tool execution.
