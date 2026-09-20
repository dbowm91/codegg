# ADR-0008: LSP Preview Apply Authorization and Runtime Ownership

Status: accepted

Date: 2026-09-20

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmaps:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md`

Related plans:

- `plans/implementation/coding-agent-tool-surface-corrective/006-lsp-preview-runtime-authority-seam.md`
- `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md`
- `plans/implementation/architecture-convergence-incomplete-verticals/007-controlled-lsp-mutation-application.md`

## Context

Architecture Convergence M007 established one daemon-owned checked mutation path for applying reviewed LSP preview artifacts. `CoreRequest::LspPreviewApply` carries bounded host-generated preview revision/digest/patch metadata plus workspace and session locators, and `src/lsp/mutation.rs::apply_preview()` owns containment, stale-hash validation, workspace locking, rollback, edit checkpoints, file-change projection, and LSP synchronization.

The coding-agent tool-surface corrective later attempted to expose that existing capability to model-facing coding agents. M005 correctly stopped because the tool-construction layer could not safely assemble the same trusted runtime state:

- `TurnRunInput` already holds the daemon-owned `LspService`, immutable `ExecutionContext`, pool, and workspace-service lease;
- `DefaultTurnRuntime` already derives the canonical workspace lock table;
- `build_session_tool_registry()` currently drops the supplied LSP service by constructing `ToolRegistryOptions` with `lsp_service: None`;
- each `LspTool` owns a private `PreviewArtifactRegistry`, preventing a sibling adapter from resolving the exact preview the model just created.

The re-audit also found an authorization mismatch. `CoreRequest::LspPreviewApply` already contains a session locator and requires semantic capability `file.modify`, but the authorization matrix classifies it as `ScopeKind::Opaque`. Under the team authorization model, opaque operations cannot resolve an owning project and therefore fail closed for non-LocalOwner principals. This makes the existing explicit TUI apply path effectively LocalOwner-only even though Contributor and higher project roles already carry `FileModify`.

A durable decision is required because correcting the transport scope changes effective authorization behavior and because the future agent-facing caller must not accidentally inherit, bypass, or duplicate the transport authorization boundary.

## Decision drivers

- Checked LSP preview application must remain one mutation implementation.
- A reviewed preview is input evidence, not mutation authority.
- Team principals with the existing `file.modify` capability should be evaluated against the preview's owning session/project rather than rejected solely because the request was classified opaque.
- Viewer/non-member/revoked principals must remain denied.
- Model-facing tools must not supply or override project, workspace, session, path, patch, digest, or hash authority material.
- Human/TUI transport authorization and model-tool authorization are different caller boundaries and must not be faked through one another.
- The daemon-resolved LSP service and workspace lock table must remain canonical.
- Preview state should remain bounded and ephemeral; M005 does not justify durable or process-global preview storage.
- Tool Programs and read-only agents must not gain this mutation capability.

## Considered options

### A. Keep `LspPreviewApply` opaque and LocalOwner-only

This preserves current effective behavior for the explicit TUI apply path, but it conflicts with the existing semantic `file.modify` capability model and prevents properly authorized team Contributors/Maintainers/Owners from applying a reviewed preview in their own project.

It also leaves M005 without a coherent parity story between human and agent callers.

Rejected.

### B. Route the future model tool through a synthetic `CoreRequest::LspPreviewApply`

This would reuse the transport authorization gate mechanically, but it conflates an internal model-tool invocation with a frontend transport request, requires synthetic request authority/client context, and risks bypassing or duplicating the ToolBroker/permission boundary that already governs direct coding-agent filesystem mutation.

Rejected.

### C. Introduce a daemon-global or workspace-global preview registry

This would make previews easy to resolve from multiple callers, but unnecessarily widens preview lifetime and visibility, complicates session isolation, restart semantics, cleanup, and cross-user privacy, and creates a new mutable daemon-owned state surface.

Rejected.

### D. Preserve caller-specific authorization and share only host-owned runtime state

Accepted.

The human/TUI path remains a native `CoreRequest` and is authorized by the canonical `AuthorizationService`. Its scope changes from `Opaque` to `ViaSession`; the nested `request.session_id` is resolved to the owning project and the existing `Capability::FileModify` requirement is enforced.

The future model-facing path remains a normal direct agent tool. It is authorized by the resolved tool surface/parent ceiling, permission decision, and verified ToolBroker mutating contract. It receives session/workspace identity only from the host session-tool construction seam. It does not create or send a fake `CoreRequest`.

Both callers converge only after their caller-specific authorization boundary on the same host session/workspace binding check and the same canonical `src/lsp/mutation.rs::apply_preview()` service.

## Decision

### Transport authorization

`CoreRequest::LspPreviewApply` is a session-scoped project mutation:

```text
operation = lsp_preview_apply
scope = via_session
capability = file.modify
```

The daemon's canonical session-locator extractor must recognize the nested `LspPreviewApplyRequestDto.session_id`. Project resolution and membership/capability evaluation remain owned by `AuthorizationService`; handlers must not hand-roll role checks.

Unknown sessions, sessions without an owning project, inactive/revoked principals, missing membership, and callers lacking `FileModify` fail closed.

### Agent authorization

A model-facing preview-apply tool, when implemented, is a direct mutating tool governed by the ordinary coding-agent authority path:

```text
resolved model tool surface / parent ceiling
  -> permission decision
  -> ToolBroker verified mutating contract
  -> host-bound session/workspace runtime
  -> shared session/workspace binding validation
  -> canonical apply_preview
```

The model-facing schema contains preview identity only. Workspace, project, session, turn, paths, patches, revision, digest, hashes, provenance, LSP service, and lock identity are host-owned.

The agent tool must not construct transport authority, impersonate a frontend client, or route through a synthetic CoreRequest.

### Runtime ownership

The session/turn tool factory owns composition of the LSP runtime inputs for direct tools:

- the daemon-resolved `Arc<LspService>`;
- canonical `ExecutionContext.workspace_id` and `workspace_root`;
- bound session/turn identity;
- durable pool when available;
- the canonical `WorkspaceLockTable` from the workspace-service lease;
- one shared bounded ephemeral preview-registry handle for the lifetime of that tool registry.

The preview registry is shared only among host components that participate in the same session/turn tool-registry lifetime. It is not process-global, daemon-global, workspace-global, serialized, or durable. Independent turn/session registries are isolated. Restart invalidates preview IDs.

Default/standalone `LspTool` construction may continue to create an isolated registry for tests and non-daemon compatibility paths.

### Mutation ownership

`src/lsp/mutation.rs::apply_preview()` remains the sole checked LSP preview mutation/checkpoint implementation. Any reusable session/workspace binding helper may be factored next to that boundary, but digest/hash validation, patch application, rollback, checkpointing, lock semantics, file projection, and LSP synchronization must not be copied into the model tool or transport handler.

## Consequences

### Positive

- Team principals are evaluated using the existing `FileModify` capability on the correct session-owned project.
- Viewer/non-member denial semantics remain explicit and fail closed.
- The model tool can use normal coding-agent permission/ToolBroker authority rather than a fake transport request.
- Model-facing LSP calls can reuse the daemon LSP service instead of silently constructing an unrelated service.
- Preview identity can be resolved safely by a sibling adapter without introducing global or durable preview state.
- Human and agent callers converge on one mutation implementation while retaining their correct caller-specific authorization boundaries.

### Negative

- Session tool construction gains explicit LSP runtime wiring and a shared in-memory preview handle.
- Authorization matrix tests/documentation must change because `lsp_preview_apply` is no longer opaque.
- Preview IDs remain turn/session-tool-registry ephemeral; a preview from a prior registry lifetime is intentionally unavailable.

### Neutral or deferred

- Project role bundles do not change.
- No new capability is introduced.
- The supported WorkspaceEdit subset does not change.
- Tool Programs remain excluded.
- Durable preview persistence and cross-turn preview reuse are deferred and would require a separate decision.

## Compatibility and migration

No storage migration, protocol shape change, or configuration migration is required.

Existing LocalOwner callers remain authorized. Existing team principals gain access only when the canonical session-owned project resolves and their active membership includes `FileModify`.

The `LspPreviewApplyRequestDto` remains unchanged. Existing TUI command names and preview artifact formats remain unchanged.

The production tool factory begins reusing the daemon-provided LSP service when available; fallback/disabled behavior remains explicit when it is not available.

## Security and reliability implications

Authorization is checked at the correct caller boundary and never inferred from preview possession.

Cross-session and cross-workspace preview use must fail through host binding checks even when a preview ID is guessed. A shared registry handle does not widen authority because registry visibility is limited to one host tool-registry lifetime and mutation still requires ToolBroker or transport authorization plus canonical binding/revalidation.

The canonical workspace lock remains mandatory for mutation. Preview metadata lookup must occur before mutation without holding unrelated locks longer than necessary.

Duplicate or uncertain apply attempts retain the existing conservative non-idempotent semantics. No automatic replay is authorized after an ambiguous post-dispatch failure unless the canonical mutation service can prove no apply occurred.

Restart drops preview state. No recovery process recreates previews from transcripts or model output.

## Verification

Conforming implementation requires evidence for:

- authorization descriptor `via_session` + `file.modify`;
- nested session-locator extraction;
- Contributor allowed, Viewer denied, inactive/revoked/non-member denied, LocalOwner compatible;
- unknown/cross-project session fail-closed behavior;
- daemon-provided `Arc<LspService>` reused by model-facing LSP/tool-registry construction;
- one shared preview-registry handle visible to sibling host components and isolated across independent registries;
- no process-global or durable preview store;
- one session/workspace binding helper used by both caller paths where applicable;
- one `apply_preview` mutation implementation;
- model schema containing preview identity only;
- Tool Programs/read-only callers denied;
- stale hash/digest, duplicate apply, restart expiry, and workspace-lock contention behavior.

## Supersession

A future durable cross-turn LSP preview system or generalized project mutation authorization framework may supersede this ADR only if it preserves caller-specific authorization, host-owned identity, fail-closed project/workspace binding, one checked mutation owner, and explicit preview-lifetime semantics.
