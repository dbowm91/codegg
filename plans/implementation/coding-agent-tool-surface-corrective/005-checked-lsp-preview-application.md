# Coding-Agent Tool Surface Corrective M005 — Agent-Facing Checked LSP Preview Application

Status: ready for handoff

Blocker: cleared by closed M006. M005 must consume the daemon-owned LSP service, shared turn-local preview registry, canonical workspace locks, host-bound session/workspace identity, and reusable binding seam established by M006; it must not bypass or duplicate authority.

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (original production audit baseline)

Blocker re-audit baseline: `5447e3bc62114cf91992690e26f3a09b897802c5`

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m005--checked-lsp-preview-application`

Hard dependencies:

- M001 is closed and supplies the canonical category/capability/discovery semantics.
- M006 `plans/implementation/coding-agent-tool-surface-corrective/006-lsp-preview-runtime-authority-seam.md` must close first. It owns daemon LSP-service reuse, the shared ephemeral preview-registry handle, session-resolved transport authorization, reusable session/workspace binding, and proof that the factory has all host dependencies required by this adapter.

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`

Predecessor capability already closed:

- `plans/implementation/architecture-convergence-incomplete-verticals/007-controlled-lsp-mutation-application.md`
- `plans/closure/architecture-convergence-incomplete-verticals/007-status.md`

Applicable ADRs:

- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted)

Primary class: capability

## 1. Objective

Expose the already-implemented controlled LSP preview-application capability to coding agents through one narrow mutating tool.

Architecture Convergence M007 already closed the difficult mutation boundary:

```text
reviewed LSP preview candidate
      ->
LspPreviewApplyRequestDto
      ->
CoreRequest::LspPreviewApply
      ->
src/lsp/mutation.rs::apply_preview
      ->
digest + per-file SHA revalidation
      ->
canonical workspace lock
      ->
atomic-enough writes / rollback-on-error
      ->
checked edit checkpoint
      ->
FileChanged projection + LSP document synchronization
```

The current model-facing `lsp` tool deliberately remains read-only. The current explicit apply surface is human/TUI-facing (`/lsp-preview-apply`) and the protocol/daemon mutation path; there is no ordinary coding-agent tool that can take a preview it just generated and request the same controlled application.

M005 must add only that missing adapter. It must **not** reimplement preview normalization, hashing, patch application, rollback, checkpointing, workspace locking, or LSP synchronization.

The preferred model contract is intentionally tiny:

```json
{"preview_id": "preview-..."}
```

All revision/digest/patch/path/provenance data must be resolved from the host-owned preview registry, not accepted from the model.

## 2. Why this milestone is ready

M001 is no longer the blocker. It closed successfully.

The post-M004 re-audit found a narrower construction/authority seam:

- `TurnRunInput` already carries the daemon-owned `LspService`, immutable `ExecutionContext`, pool, project/repository identifiers, and workspace-service lease.
- `DefaultTurnRuntime` already derives the canonical `WorkspaceLockTable`.
- `SessionToolContext` already carries locks and turn/project context.
- `build_session_tool_registry()` nevertheless passes `lsp_service: None` into `ToolRegistryOptions`, so the model-facing LSP tool is not explicitly constructed over the daemon service used by the turn.
- `LspTool` owns a private `PreviewArtifactRegistry`; a sibling apply tool cannot resolve the preview ID without a shared host handle.
- `CoreRequest::LspPreviewApply` is currently `ScopeKind::Opaque` despite carrying a session ID, so team principals cannot traverse the normal session-owned-project `file.modify` authorization path.
- The daemon handler's session/workspace binding SQL check is not yet a reusable internal seam.

M006 closed with the required runtime composition, shared preview handle,
session-scoped authorization, and reusable session/workspace binding seam. M005
can now add only the model-facing adapter over those installed owners.

## 3. Current implementation evidence

At the audited baseline:

- `crates/codegg-protocol/src/lsp.rs` defines bounded `LspPreviewApplyRequestDto` and `LspPreviewApplyResultDto`.
- `CoreRequest::LspPreviewApply` / `CoreResponse::LspPreviewApplyResult` are additive protocol paths.
- `src/core/daemon_goals.rs` owns the daemon request-family dispatch for LSP preview application.
- `src/lsp/mutation.rs::apply_preview()` is explicitly the daemon-owned reviewed-LSP-workspace-edit mutation boundary.
- The mutation service validates request shape, canonical workspace containment, preview digest, duplicate paths, per-file original SHA, and patch application.
- It holds `WorkspaceLockTable`, captures pre/post states through `EditCheckpointManager`, writes atomically per file, rolls back prior writes on failure, persists one checked edit checkpoint, emits `FileChanged`, and updates open LSP documents.
- Supported preview kinds are currently bounded to `rename`, `code_action`, and `formatting`; resource operations/opaque commands remain denied by the predecessor capability.
- `PreviewArtifactRegistry` entries carry preview revision/digest and apply metadata; `export_preview_apply_candidate()` is a read-only handoff used by the TUI apply flow.
- `/lsp-preview-apply` is an explicit human command and observer/project authorization tests already cover the protocol mutation path.
- The model-facing `lsp` tool is `ReadOnly`; Tool Programs intentionally cannot invoke LSP preview application.

Therefore M005 is not a new checked-edit feature. It is a model-facing invocation adapter over an existing checked-edit feature.

## 4. Invariants that must not regress

- `lsp` preview generation stays `ToolCategory::ReadOnly`; no preview-producing call writes files.
- `src/lsp/mutation.rs` remains the canonical LSP-preview mutation/checkpoint owner.
- The agent-facing tool is a separate `Mutating`/filesystem-write capability and passes normal ToolBroker/permission/parent-ceiling policy.
- The model cannot supply or override patches, paths, hashes, preview revision, preview digest, provenance, workspace identity, session identity, or turn identity.
- Preview identity is scoped to the owning session/workspace; cross-session/project/workspace use fails closed.
- The adapter cannot bypass project/session/tool/workspace authorization merely because it runs inside the daemon.
- Existing stale digest/hash/path checks, workspace lock, rollback, checkpoint persistence, FileChanged projection, and LSP synchronization are reused rather than copied.
- Opaque LSP commands and `workspace/executeCommand` remain denied.
- Tool Programs/verifier/read-only agents do not gain this mutation capability by discovery.
- A previously applied or no-longer-present preview is not blindly replayed.
- No new durable preview store is created.

## 5. Scope

### In scope

- A native model tool such as `lsp_preview_apply` (exact name follows repo conventions).
- Input restricted to an opaque preview identifier, plus at most a host-verifiable expected revision if the existing registry contract requires it.
- Host-side export/lookup of the current preview candidate from the same shared `LspService`/preview registry used by the model-facing preview call.
- Construction of the canonical `LspPreviewApplyRequestDto` from host-owned data.
- Delegation to the existing `src/lsp/mutation.rs` mutation service through the smallest reusable service seam.
- Registration wiring with the current session/workspace/pool/`WorkspaceLockTable`/shared LSP service.
- Correct ToolContract/category/effect/idempotency/retry semantics.
- Structured projection of the existing `LspPreviewApplyResultDto`.
- Regression tests comparing TUI/protocol and model-tool application semantics.
- Documentation and tool-surface/profile integration.

### Explicitly out of scope

- Reimplementing `apply_preview`.
- Changing the supported WorkspaceEdit subset.
- Adding resource create/rename/delete operations.
- Enabling LSP commands or `workspace/executeCommand`.
- Persisting preview registry entries across restart.
- New edit-history/checkpoint storage.
- New conflict/fuzzy-merge behavior.
- Letting the model send raw patches through this tool; `apply_patch` already owns model-supplied patch mutation.
- Making preview application available to Tool Programs in this milestone.
- Replacing the human `/lsp-preview-apply` path.

## 6. Required production changes

### Agent-tool adapter

Add a small native tool whose public parameters do not reproduce `LspPreviewApplyRequestDto`. The DTO is an internal trusted handoff object containing host-owned material.

Illustrative public schema:

```json
{
  "type": "object",
  "properties": {
    "preview_id": {
      "type": "string",
      "description": "Opaque LSP preview identifier returned by a previous preview operation"
    }
  },
  "required": ["preview_id"],
  "additionalProperties": false
}
```

The adapter should:

1. obtain the current shared preview entry/candidate by ID;
2. verify it belongs to the current session/workspace execution context and is eligible for agent apply;
3. reject absent, stale-marked, unsupported, command-bearing, or already-applied candidates before invoking mutation;
4. construct the full canonical request from host-owned revision/digest/kind/title/provenance/workspace/session/turn/patches;
5. call the same canonical mutation service used by the daemon path;
6. mark the preview applied only after mutation success, preserving the predecessor's lifecycle behavior;
7. return the existing typed result fields in bounded structured form.

### Reusable service seam

Do not route a tool call through a fake TUI command or duplicate the `CoreRequest` dispatcher. Prefer the smallest internal service/facade that both daemon request handling and the new tool can call while preserving authorization context.

If `src/lsp/mutation.rs::apply_preview` is already that reusable seam, inject its required dependencies into the tool:

- canonical workspace root;
- shared `WorkspaceLockTable`;
- database pool/checkpoint store;
- shared `LspService`;
- bound session/workspace/turn identity.

If tool construction lacks one dependency (notably workspace locks), thread it through `SessionToolContext`/`ToolRegistryOptions` rather than introducing a global.

### Authorization boundary

M006 establishes two caller-specific authority paths that converge only after authorization:

```text
Human/TUI:
transport principal
  -> AuthorizationService
  -> file.modify via session-owned project
  -> host session/workspace binding
  -> canonical apply service

Agent:
resolved model tool surface / parent ceiling
  -> permission decision
  -> ToolBroker verified mutating contract
  -> host-bound session/workspace runtime from M006
  -> host session/workspace binding
  -> canonical apply service
```

M005 must use the second path. Do not route the model tool through a fake TUI/CoreRequest envelope and do not invent a second project authorization system.

The adapter must receive workspace/session identity only from the M006 host construction seam. Model-provided session/project/workspace IDs are forbidden.

### Contract semantics

Classify the tool as filesystem mutation. It is non-idempotent at the write boundary unless the existing preview applied-state makes duplicate invocation safely recognizable. Retry policy must therefore be conservative: no automatic replay after an uncertain post-dispatch failure.

Output schema should wrap/reuse `LspPreviewApplyResultDto` fields, including:

- preview ID/revision/digest;
- kind/title;
- written files;
- checkpoint ID;
- bounded synchronization warnings.

## 7. Ordered work packages

### Work package A — Authority and dependency trace

Document the TUI/protocol path and the model ToolBroker path side by side. Identify the trusted session/workspace/project/principal fields available to the tool and the dependencies required by `apply_preview`.

Acceptance evidence: one explicit diagram shows no authority step silently disappears.

### Work package B — Shared apply service seam

If needed, extract only enough construction/service glue so the existing daemon handler and model tool call the same canonical apply implementation. Do not move validation/write/checkpoint logic into the tool.

Acceptance evidence: repository search shows one implementation of digest/hash validation and checked application.

### Work package C — Host-owned preview lookup and request construction

Resolve the preview candidate from the shared LSP preview registry and build the canonical DTO internally.

Acceptance evidence:

- model schema contains no patch/path/hash/digest/session/workspace override fields;
- tampered/unknown/cross-session preview IDs fail before mutation;
- the internal DTO exactly matches the candidate the user-facing TUI path would apply.

### Work package D — Tool contract and retry semantics

Register the tool with M001 canonical category/capability metadata, appropriate caller policy, non-idempotent/uncertain-outcome handling, and structured output schema.

Acceptance evidence: read-only/verifier/program callers remain denied; mutation-capable direct coding agent can request permission and apply.

### Work package E — Equivalence and negative tests

Run the same representative rename/formatting/edit-only code-action preview through both existing protocol/TUI preparation and the model adapter, then compare mutation result/checkpoint/file contents.

Acceptance evidence: both routes converge on the same `apply_preview` service and stale/invalid behavior.

### Work package F — Documentation/exposure

Update LSP/tool docs to state:

- preview creation is read-only;
- human explicit apply is `/lsp-preview-apply`;
- agent explicit apply is the new mutating tool;
- both delegate the same checked mutation service;
- no opaque server command execution is enabled.

## 8. Failure, cancellation, restart, and contention semantics

These inherit the closed M007 mutation service.

- unknown/expired preview: no mutation;
- unsupported/already-applied preview: no mutation;
- stale digest/file hash: no mutation;
- path/containment failure: no mutation;
- permission/authority denial: no mutation;
- mutation write/post-state/checkpoint failure: existing rollback/error semantics;
- LSP synchronization failure after committed checkpoint remains a bounded warning as currently defined;
- concurrent apply uses the existing repository workspace lock;
- duplicate preview application must not replay writes blindly;
- restart invalidates ephemeral preview IDs if that remains canonical behavior; return explicit not-found/expired rather than adding persistence;
- uncertain post-dispatch tool failure must not trigger automatic retry unless the canonical service can prove the preview was not applied.

## 9. Compatibility and migration

Additive model tool only. No storage migration, protocol version bump, or change to `LspPreviewApplyRequestDto` is expected.

Existing TUI/protocol clients continue unchanged. Existing models may continue translating previews into `apply_patch`; the new adapter is the safer semantic shortcut.

## 10. Required tests

### Focused unit tests

- public schema exposes preview ID only;
- host-owned candidate -> DTO conversion;
- unknown/already-applied/unsupported preview rejection;
- tool category/effect/retry contract;
- shared-service wiring uses current session/workspace context.

### Integration tests

- model `renamePreview` -> `lsp_preview_apply` -> expected file content + checkpoint;
- formatting preview apply;
- edit-only code-action apply;
- result fields match canonical `LspPreviewApplyResultDto`;
- same candidate applied through existing CoreRequest route and model route produces equivalent terminal state in independent fixtures.

### Security/negative tests

- model attempts to pass path/patch/hash/digest/session/workspace fields are rejected by schema;
- cross-session/project/workspace preview use denied;
- read-only parent/verifier/program caller denied;
- stale file after preview rejected;
- opaque command/mixed unsupported action remains denied;
- observer/non-controller project authority remains consistent with existing LSP mutation authorization.

### Contention/cancellation tests

- two callers race the same preview: at most one successful apply;
- concurrent workspace mutation revalidation/lock behavior remains M007-correct;
- no automatic replay after simulated uncertain post-dispatch outcome.

### Restart/recovery tests

- pre-restart ephemeral preview ID fails explicitly after restart if registry is not durable;
- no new preview persistence files/rows appear.

## 11. Required verification commands

Use current exact targets at implementation time. Minimum expected:

```bash
cargo test -p codegg --lib tool::lsp
cargo test -p codegg --lib lsp::mutation
cargo test -p egglsp --lib preview_registry
cargo test --test lsp
cargo test --test presence_m003_observation
cargo test --test tool_execution
cargo test --test edit_checkpoint
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Reuse the existing Architecture Convergence M007 regression matrix where possible rather than creating a parallel LSP mutation suite.

## 12. Documentation updates

- `architecture/lsp.md`: two explicit apply callers, one checked mutation owner.
- `architecture/tool.md`: agent-facing adapter, mutating category, structured result.
- `architecture/agent-tool-surface.md`: disclosure/capability classification.
- `architecture/core.md`: only if service extraction changes the documented daemon-family implementation seam; CoreRequest ownership itself remains unchanged.

## 13. Acceptance criteria

M005 closes when:

1. a mutation-capable coding agent can apply an LSP preview it received using only its opaque preview ID;
2. all patch/path/hash/revision/digest/session/workspace data comes from trusted host preview state;
3. the model adapter and existing human/protocol path call the same canonical `src/lsp/mutation.rs` checked application logic;
4. stale/unsupported/cross-session/read-only/command-only cases fail without mutation;
5. successful application creates the existing checked edit checkpoint and LSP synchronization behavior;
6. no new mutation, storage, scheduler, or authorization owner exists.

## 14. Stop conditions

Stop and report if:

- M006 is not strictly closed;
- current ToolExecutionContext/session construction cannot provide enough trusted authority to call the mutation service without bypassing daemon/project authorization;
- implementation would need to duplicate `src/lsp/mutation.rs` validation/write/checkpoint logic;
- model access requires exposing raw patches/digests or CoreRequest identity fields;
- supporting a preview class requires expanding the closed M007 WorkspaceEdit subset or enabling `workspace/executeCommand`;
- preserving preview IDs across restart would require new persistence.

## 15. Closure evidence required

Include:

- side-by-side human/protocol vs model-tool authority/call trace;
- proof of one mutation implementation owner;
- public model schema showing preview-ID-only input;
- host candidate -> DTO construction evidence;
- rename/format/code-action success trajectories;
- stale/cross-session/read-only/program/opaque-command negatives;
- duplicate-race and uncertain-retry evidence;
- exact checkpoint/result equivalence;
- verification commands/results;
- residual unsupported preview classes;
- registry closure disposition.

## 16. Handoff notes

The difficult mutation capability is already implemented and closed, and M006 is the only prerequisite seam this adapter should consume. Resist rebuilding either layer. M005 is successful when the agent adapter is boring: accept a preview ID, resolve the M006 shared host state, pass normal ToolBroker mutation authority, invoke the canonical service, mark the shared preview applied only after success, and project its typed result.
