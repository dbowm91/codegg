# Runtime Consolidation, Deletion, and Footprint M003 Corrective Pass — Physical Extraction

Status: implemented — closure accepted by M009

Source plan: `plans/implementation/runtime-consolidation-deletion-footprint/003-agent-loop-ownership-decomposition.md`

Source closure: `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`

Primary class: infrastructure / maintainability with correctness preservation

## Objective

Physically move the remaining permission/dispatch/snapshot batch implementation
and context policy methods out of `src/agent/loop.rs` into the concrete owners
already introduced by M003, then rerun strict M003 closure evidence.

## Scope

- move tool-batch helper and execution bodies into `tool_batch.rs`;
- move context observation/palette/cache policy bodies into `context_runtime.rs`;
- retain existing broker, permission, MCP, snapshot, cancellation, ordering,
  recovery, and PromptCompiler contracts;
- remove delegation-only duplication and update architecture documentation.

Out of scope: provider clients, public protocol, storage, scheduler authority,
new traits/frameworks, and unrelated naming cleanup.

## Acceptance and verification

`loop.rs` must be materially smaller, no extracted module may recreate the
multi-domain god file, focused loop/recovery/harness tests must pass, and the
exact M003 verification commands plus locked workspace Clippy must pass.

## Stop condition

Stop for a new ADR if physical extraction requires changing public protocol,
storage, scheduler authority, or provider API contracts.
