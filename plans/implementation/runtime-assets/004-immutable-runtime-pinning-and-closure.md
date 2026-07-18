# Runtime Assets Milestone 004 — Immutable Runtime Pinning and Closure

Status: ready for handoff

Repository baseline: `972c286` (Runtime Assets Milestone 003 closed)

Source roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-4--immutable-runtime-pinning-and-closure`

Related closure evidence:

- `plans/closure/runtime-assets/003-status.md`

Primary class: invariant

## 1. Objective

Complete the remaining Runtime Assets invariant work after the refresh
coordinator: make runtime/resource references auditable and bounded for the
full turn/agent-run lifetime, define the inert remote-manifest seam, and
close the subsystem with deterministic security, restart, and compatibility
evidence.

## 2. Dependencies and boundaries

Hard dependencies Runtime Assets Milestones 001–003 are closed. This plan
consumes the daemon refresh coordinator and immutable turn snapshot capture;
it must not reopen or duplicate those mechanisms.

It must not implement file watchers, distributed transport, project catalog
activation leases, TUI tabs, provider authorization, or skill/script
execution. Remote manifest types are data-only compatibility seams.

## 3. Scope

### In scope

- Capture generation/fingerprint and activated-skill digests at turn and
  agent-run boundaries where the existing run metadata can carry them.
- Add bounded local resource handles with canonical containment checks and
  size/read limits; no eager unbounded skill-resource reads.
- Define inert remote workspace-manifest DTOs with provenance, digest, and
  compatibility diagnostics, without transport or synchronization writes.
- Add restart, resource-boundary, symlink/traversal, active-turn pinning, and
  compatibility tests.
- Document the final Runtime Assets authority and closure/rollback behavior.

### Explicitly out of scope

- Watcher-driven correctness, distributed manifests, remote execution,
  project activation policy, and multi-project UI.
- Mutating snapshots or changing an in-flight turn.
- Automatic execution of any bundled skill or harness script.

## 4. Required evidence

- A turn/agent run retains its captured generation and fingerprint after
  subsequent refreshes.
- Resource handles reject traversal, symlink escape, oversized, and malformed
  reads within bounded diagnostics.
- Restart reconstructs equivalent effective asset identity from explicit
  context and metadata.
- Remote manifest DTOs serialize as inert bounded data and cannot authorize
  local execution.
- Full focused asset, protocol, storage, security-guard, and capped workspace
  evidence is recorded in a new closure record.

## 5. Verification and closure

Use the repository's capped workspace test command, the asset/protocol/TUI
focused suites, all required static guards, and adversarial resource fixtures.
Create `plans/closure/runtime-assets/004-status.md` only after all findings
are resolved or explicitly conditioned with exact external evidence.
