# Agent Runtime, Model Adaptation, and ACP Milestone 002 — Closure

Status: closed

Source plan: `plans/implementation/agent-runtime-model-adaptation-acp/002-resolved-capability-and-tool-surface.md`

## Outcome

Implemented and integrated the typed `AgentCapabilitySet` and immutable
`ResolvedToolSurface`. Model-facing definitions pass through one deterministic
resolver after native exposure/model/plan filtering. The resolver records
omissions, backend kind, canonical/wire mappings, authority capabilities, and
a stable fingerprint. Task is advertised only when its implementation has a
functional spawner or scheduler submission backend.

The worker read-only path no longer infers authority from agent role or name;
it uses explicit permission denies. Delegation is independently represented,
so a read-only parent can retain a callable task surface when policy permits
delegation. Parent capability intersection and alias-aware resolution are
available for Milestones 003 and 007.

## Closure evidence

- Filtering/schema inventory: model and plan exposure remain in
  `src/agent/loop.rs`, profile disables remain in profile policy, and MCP
  definitions are combined with native definitions before resolution. The
  resolver is the final model-facing surface and owns task callability,
  canonical/wire maps, omissions, capabilities, and fingerprint.
- Capability matrix: `AgentCapabilitySet` covers filesystem read/write, shell
  read/mutate, Git read/write, research network, delegation, todo/goal state,
  terminal, and image. `intersect` is field-wise AND and cannot widen a
  parent ceiling.
- Canonical/wire fixtures: alias-aware resolution preserves both maps and
  rejects canonical/wire collisions; fingerprint tests prove order
  independence.
- Agreement evidence: production root/child prompt tool lists and provider
  definitions are produced from the resolved surface used for capability and
  omission diagnostics. Explicit agent deny entries and model disables feed
  resolution.
- Read-only delegator evidence: task contributes only `Delegate`, not
  filesystem mutation authority, and is no longer selected by role/name.
- Palette restoration evidence: `reduce` selects from immutable `tools`,
  while `definitions()` reconstructs the complete base surface.
- Diagnostics are bounded to names, categories/backend kinds, counts,
  capabilities, omission reasons, and the fingerprint; schemas and arguments
  are not logged.

## Verification

Passed locally:

```text
cargo fmt --all
cargo check --workspace
cargo test -p codegg --lib agent::tool_surface
cargo test -p codegg --lib agent::policy
cargo test --test tool_contract_guards
```

Adapter TOML parsing, specialized workflows, and durable nested-agent
implementation remain correctly deferred to later milestones.

## Dependency disposition

Milestones 003, 006, and 007 are now dependency-ready. Milestone 003 is the
next strict handoff; Milestone 006 may begin against this surface but cannot
close its nested-agent integration until Milestone 003 closes. Milestones
004/005 remain behind 003, 008 behind 007, 009 behind 007, and 010/011 retain
their existing predecessor closures.
