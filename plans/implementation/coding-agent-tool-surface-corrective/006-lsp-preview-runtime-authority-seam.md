# Coding-Agent Tool Surface Corrective M006 — LSP Preview Runtime and Authority Seam

Status: ready for handoff

Repository baseline: `5447e3bc62114cf91992690e26f3a09b897802c5`

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m006--lsp-preview-runtime-and-authority-seam`

Related blocked capability:

- `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md`
- `plans/closure/coding-agent-tool-surface-corrective/004-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`

Applicable ADRs:

- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted)

Primary class: invariant

## 1. Objective

Create the missing trusted runtime seam required by M005 without adding the model-facing mutation tool yet.

The milestone must make one session/turn tool registry use:

- the daemon-resolved `LspService`;
- one shared ephemeral preview registry for the model-facing `lsp` tool and a future apply adapter;
- the already-available canonical workspace lock table and immutable workspace/session identity;
- a transport authorization classification for `CoreRequest::LspPreviewApply` that resolves its existing session locator to the owning project instead of treating the request as opaque.

The outcome is infrastructure only. M006 does not expose a new mutating model tool and does not call `apply_preview` from the agent surface.

## 2. Why this milestone is ready

M001 is strictly closed, so native tool category/capability/discovery semantics are stable.

The M004 unblock audit stopped M005 because the tool layer could not prove the required workspace/session state or share the preview candidate. Repository inspection at `5447e3bc` resolves the blocker into concrete wiring defects rather than an unresolved architecture choice:

1. `TurnRunInput` already carries the daemon-owned `lsp_service`, immutable `ExecutionContext`, database pool, project/repository identifiers, and `workspace_service_lease`.
2. `DefaultTurnRuntime` already derives the canonical `WorkspaceLockTable` from the workspace service lease.
3. `SessionToolContext` already carries the workspace locks, project/repository IDs, and turn ID.
4. `build_session_tool_registry()` currently constructs `ToolRegistryOptions` with `lsp_service: None`, so the model-facing LSP registry is not explicitly bound to the daemon LSP service used for turn context.
5. `LspTool` owns a private `Mutex<PreviewArtifactRegistry>`, so a sibling adapter cannot resolve the exact preview produced by that instance.
6. `CoreRequest::LspPreviewApply` already contains `request.session_id`, but `operation_descriptor()` classifies it as `ScopeKind::Opaque` and `session_id_for_request()` does not extract the nested session ID. Team principals therefore cannot reach the normal project-scoped `file.modify` authorization path even though Contributor and higher roles carry `FileModify`.
7. `src/lsp/mutation.rs::apply_preview()` remains the complete checked mutation owner and requires no replacement.

These are all local, additive seams with existing owners.

## 3. Current implementation evidence

### Turn/runtime state already exists

`src/agent/turn_runtime.rs` receives:

- `lsp_service: Option<Arc<LspService>>`;
- `execution: Arc<ExecutionContext>` with canonical `workspace_id` and `workspace_root`;
- `workspace_service_lease`, from which the canonical lock table is derived;
- `pool`, `project_id`, `repository_id`, `session_id`, and `turn_id`.

The same function passes `workspace_locks` into `SessionToolContext`, but does not pass `lsp_service`.

### Tool registry currently loses daemon LSP ownership

`src/tool/factory.rs::build_session_tool_registry()` sets:

```text
ToolRegistryOptions {
    ...
    lsp_service: None,
    ...
}
```

`ToolRegistry::with_options()` can already consume `ToolRegistryOptions.lsp_service` and shares that service between model-facing `lsp` and hidden `lsp_read`. The missing piece is production factory wiring.

### Preview registry is instance-private

`src/tool/lsp.rs::LspTool` stores:

```text
preview_registry: parking_lot::Mutex<PreviewArtifactRegistry>
```

and constructs a fresh registry in `with_cache_config()`. Preview export/marking APIs already exist; ownership must become shareable without becoming process-global.

### Transport authorization is misclassified

`crates/codegg-core/src/authorization/policy.rs` currently declares:

```text
LspPreviewApply -> ScopeKind::Opaque + Capability::FileModify
```

while the DTO already contains a session ID and the daemon already knows how to resolve `ViaSession` scope to the owning project. This is an authorization-scope defect, not a reason to invent a new capability.

### Mutation ownership is already complete

`src/core/daemon_goals.rs` validates workspace/session binding, acquires the workspace service lease, and delegates to `src/lsp/mutation.rs::apply_preview()`. That service owns digest/hash validation, containment, locking, rollback, checkpointing, `FileChanged`, and LSP synchronization.

## 4. Invariants that must not regress

- `src/lsp/mutation.rs::apply_preview()` remains the sole checked LSP preview mutation implementation.
- Preview-producing `lsp` operations remain read-only.
- The preview registry remains ephemeral and bounded; no database/file persistence is added.
- No process-global preview registry is introduced.
- Preview identity remains scoped to the tool-registry/session-turn lifetime that created it.
- The model never supplies workspace, project, session, path, patch, digest, or hash authority material.
- The daemon-owned `WorkspaceLockTable` remains the lock owner.
- The TUI/protocol path remains authorized by `AuthorizationService`; changing `Opaque` to `ViaSession` must narrow scope correctly rather than bypass authorization.
- The future model tool remains authorized through the ordinary ToolBroker/permission/effect boundary used by other direct filesystem-mutating tools; M006 must not create a second agent authorization system.
- Local-owner behavior remains compatible.
- Viewer/read-only principals remain unable to apply previews.
- Unknown/revoked/cross-project session locators fail closed.
- Tool Programs do not receive a mutation path.

## 5. Scope

### In scope

- Thread the daemon-resolved `LspService` through `SessionToolContext` into `ToolRegistryOptions`.
- Introduce a cloneable shared preview-registry handle with explicit session/turn lifetime ownership.
- Allow `LspTool` construction to use that shared handle while preserving standalone/default constructors for tests and compatibility.
- Have `build_session_tool_registry()` create one shared preview registry and pass it to the model-facing `LspTool`.
- Retain the shared handle in factory scope so M005 can register a sibling adapter without downcasting the registry or inventing a global.
- Correct `LspPreviewApply` transport authorization from opaque scope to session-resolved project scope.
- Extend the canonical nested session-locator extractor for `CoreRequest::LspPreviewApply`.
- Extract/reuse only the minimum session/workspace binding helper needed so the daemon path and future agent adapter do not copy SQL binding validation.
- Update authorization/LSP/tool architecture documentation.
- Add regression tests proving service identity, preview-handle sharing, scope resolution, and fail-closed behavior.

### Explicitly out of scope

- Registering `lsp_preview_apply` as a model tool. That remains M005.
- Moving mutation logic out of `src/lsp/mutation.rs`.
- Changing the supported WorkspaceEdit subset.
- Enabling `workspace/executeCommand`.
- Persisting preview IDs across turns or restart.
- Making preview registries daemon-global or workspace-global.
- Changing project role definitions or adding a new capability.
- Reworking generic ToolBroker authority or permission semantics.
- Expanding LSP process lifecycle beyond reusing the already-resolved service.

## 6. Required production changes

### A. Thread the canonical LSP service into tool construction

Add `lsp_service` to `SessionToolContext` or an equivalently narrow runtime bundle and pass `TurnRunInput.lsp_service` through `DefaultTurnRuntime`.

`build_session_tool_registry()` must forward that service into `ToolRegistryOptions.lsp_service`.

When the daemon supplied a service, model-facing `lsp`, hidden `lsp_read`, and turn-context LSP collection must point at the same `Arc<LspService>`.

When no daemon service exists, preserve the current explicit fallback/disabled behavior. Do not consult a process-global slot.

### B. Make preview-registry ownership shareable but local

Replace the instance-private ownership shape with a cloneable handle, for example:

```text
Arc<parking_lot::Mutex<PreviewArtifactRegistry>>
```

Exact type alias/module placement is implementation-defined.

Requirements:

- default `LspTool::new`/test construction still creates an isolated registry;
- production factory construction creates one handle per session/turn tool registry;
- `LspTool` accepts the handle explicitly;
- the handle is not serialized, persisted, or stored in a global;
- registry access and mark-applied semantics remain identical.

### C. Correct transport scope for LSP preview apply

Change the authorization descriptor to:

```text
lsp_preview_apply -> ScopeKind::ViaSession + Capability::FileModify
```

and teach the canonical session-locator extractor to return `request.session_id` for `CoreRequest::LspPreviewApply`.

Do not special-case roles in the handler. The standard authorization service must decide membership/capability.

### D. Centralize session/workspace binding validation

The daemon handler currently performs a direct SQL lookup to prove the request session is bound to the requested workspace before calling `apply_preview`.

Move that validation into a reusable internal helper/service seam owned by the LSP mutation boundary or another existing canonical workspace/session service. Both the current daemon handler and M005 must be able to call it.

The helper must:

- use trusted host-provided session/workspace identifiers;
- fail closed on missing session, missing workspace binding, mismatch, storage error, or malformed identity;
- not accept project/session/workspace identity from model parameters;
- not acquire a second independent lock table.

### E. Preserve caller-specific authorization boundaries

Document and test the intended convergence:

```text
Human/TUI caller
  -> CoreRequest authorization
  -> file.modify via session-owned project
  -> session/workspace binding
  -> canonical apply_preview

Agent caller (M005, later)
  -> resolved tool surface/capability ceiling
  -> permission decision
  -> ToolBroker verified mutating contract
  -> host-bound session/workspace runtime
  -> session/workspace binding
  -> canonical apply_preview
```

M006 implements only the left-side scope correction and shared host seam. It does not add the future agent caller.

## 7. Ordered work packages

### Work package A — Runtime identity/service trace

Thread the exact `Arc<LspService>` already present in `TurnRunInput` into session tool construction.

Acceptance evidence:

- a production-style turn fixture proves prompt-context collection and model-facing `lsp` reference the same service instance;
- hidden `lsp_read` still shares that service;
- no second default service is created when one was supplied.

### Work package B — Shared preview registry

Introduce the explicit shared registry handle and production factory wiring.

Acceptance evidence:

- a preview registered through the model-facing `LspTool` is visible through a sibling holder of the same handle;
- a separately constructed turn/session registry cannot see it;
- default standalone tools remain isolated.

### Work package C — Authorization scope correction

Change `LspPreviewApply` to `ViaSession`, extend session extraction, and update the authorization matrix/docs.

Acceptance evidence:

- Contributor with active project membership and `FileModify` reaches handler validation;
- Viewer is denied;
- missing/revoked membership is denied;
- unknown session fails closed;
- a session in another project cannot be used to acquire authority for the wrong project;
- LocalOwner remains authorized.

### Work package D — Shared session/workspace binding helper

Factor the existing binding check without moving mutation logic.

Acceptance evidence:

- daemon `CoreRequest::LspPreviewApply` uses the shared helper;
- repository search shows one canonical session/workspace binding implementation for this apply path;
- failure taxonomy remains explicit and no mutation occurs before successful binding.

### Work package E — M005 construction seam proof

Add a focused non-model test/helper proving that, at the end of factory construction, all future M005 dependencies are simultaneously available in one host-owned scope:

- pool;
- canonical workspace ID/root;
- session/turn ID;
- canonical `WorkspaceLockTable`;
- daemon `LspService`;
- shared preview registry.

Do not register a model mutation tool in M006.

### Work package F — Documentation and guard reconciliation

Update:

- `architecture/lsp.md`;
- `architecture/tool.md`;
- `architecture/authorization.md`;
- `architecture/agent-tool-surface.md` only as needed to describe the blocked future adapter.

Update any authorization/static-matrix tests that intentionally pin `lsp_preview_apply` as opaque.

## 8. Failure, cancellation, restart, and contention semantics

M006 introduces no new mutation execution.

- Missing daemon LSP service: preserve current explicit unavailable/fallback semantics; never fabricate a shared daemon service identity.
- Missing workspace service lease/locks: M005 remains unavailable; M006 must expose the absence rather than synthesize locks.
- Missing durable pool: transport apply retains its existing error; M005 will remain unregistered in that context.
- Preview registry poisoning is not applicable to `parking_lot::Mutex`; registry operations remain synchronous/bounded.
- Turn/session teardown drops the shared preview handle naturally.
- Restart invalidates previews.
- Authorization lookup/storage failure fails closed for team principals.
- No lock is held while performing authorization or registry metadata lookup unless already required by existing mutation code.

## 9. Compatibility and migration

No storage migration or protocol shape change is expected.

The only authorization-matrix behavior change is intentional: `LspPreviewApply` becomes project-resolvable through its already-existing session locator. This enables properly authorized team members with `FileModify` while preserving denial for viewers/non-members.

Existing LocalOwner callers, DTOs, TUI command names, preview IDs, and LSP schemas remain compatible.

## 10. Required tests

### Focused runtime tests

- daemon-supplied `LspService` reaches model-facing `lsp`;
- `lsp` and `lsp_read` share the supplied service;
- shared preview registry round-trip across holders;
- separate registry/turn isolation;
- default constructor isolation.

### Authorization tests

- authorization matrix reports `via_session` + `file.modify`;
- nested `LspPreviewApply.request.session_id` is extracted;
- Contributor allowed with correct membership;
- Viewer denied;
- revoked/suspended/non-member denied;
- unknown session denied/missing-scope;
- cross-project mismatch fails closed;
- LocalOwner compatibility.

### Binding tests

- valid session/workspace passes;
- wrong workspace fails;
- missing session fails;
- malformed workspace identity fails;
- storage error is surfaced without mutation.

### Regression tests

- existing TUI `/lsp-preview-apply` path still reaches the same daemon mutation service;
- preview generation remains read-only;
- observer TUI mutation block remains intact;
- existing LSP preview and mutation tests remain green.

## 11. Required verification commands

Use current exact target names at implementation time. Minimum expected:

```bash
cargo test -p codegg --lib tool::lsp
cargo test -p egglsp --lib preview_registry
cargo test -p codegg-core authorization
cargo test --test lsp
cargo test --test identity_m003_daemon_authorization
cargo test --test presence_m003_observation
cargo test --test tool_registry
cargo test --test tool_execution
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

If target names have moved, use the current repository equivalent and record the substitution in closure evidence.

## 12. Documentation updates

- `architecture/lsp.md` — daemon-service reuse, shared per-turn preview registry, one mutation owner.
- `architecture/authorization.md` — `lsp_preview_apply` is `via_session` + `file.modify`.
- `architecture/tool.md` — LSP runtime inputs threaded through session tool construction.
- `architecture/core.md` only if the reusable binding helper changes the documented daemon-family seam.

## 13. Acceptance criteria

M006 closes when:

1. production turn construction passes the daemon-owned LSP service into the model tool registry;
2. the model-facing `LspTool` uses an explicit shared preview registry that a sibling future adapter can consume;
3. preview registries remain isolated across independent turn/session registries and disappear on teardown/restart;
4. `CoreRequest::LspPreviewApply` resolves authorization through its session-owned project and `FileModify`, not opaque LocalOwner-only scope;
5. the session/workspace binding check is reusable without copying mutation logic;
6. one factory/runtime scope demonstrably has every dependency M005 needs;
7. no model mutation tool, new persistence owner, global preview state, or second authorization owner was added.

## 14. Stop conditions

Stop and report rather than improvise if:

- sharing the preview registry requires daemon-global state;
- the daemon-resolved LSP service cannot be reused by the tool registry without changing LSP process ownership;
- implementation would violate ADR-0008's caller-specific authorization, turn-local preview lifetime, or single mutation-owner decision;
- agent mutation would require changing project-role capability semantics;
- the session/workspace binding cannot be factored without moving mutation ownership out of `src/lsp/mutation.rs`;
- M005 would still require model-supplied workspace/session/project identity after this seam lands;
- preview persistence across turns/restarts becomes necessary.

## 15. Closure evidence required

Include:

- exact runtime ownership diagram from `TurnRunInput` to `LspTool`;
- `Arc<LspService>` identity evidence;
- shared preview-registry identity and isolation evidence;
- authorization descriptor before/after and team-role trajectories;
- session/workspace binding helper evidence;
- proof that `apply_preview` remains the sole mutation implementation;
- exact verification command results;
- an explicit M005 unblock audit.

## 16. Handoff notes

This plan exists because the M005 implementation correctly stopped. Do not work around that stop by downcasting `ToolRegistry`, constructing a second LSP service, using a process-global preview registry, or routing a model tool through a fake TUI/CoreRequest. The correct fix is to make the already-trusted runtime state meet at the session tool factory boundary.
