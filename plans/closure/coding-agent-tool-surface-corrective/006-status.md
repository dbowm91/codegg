# Coding-Agent Tool Surface Corrective M006 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/coding-agent-tool-surface-corrective/006-lsp-preview-runtime-authority-seam.md`
Source subsystem roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m006--lsp-preview-runtime-and-authority-seam`
Repository baseline reviewed: `4cf35238` (implementation commit; final verification also included planning-only closure updates)
Implementation commits: `4cf35238` — establish checked preview runtime seam; `5a88a358` — begin closure review

## 1. Executive finding

M006 is strictly closed. The daemon-resolved LSP service now reaches session
tool construction, the model-facing LSP tool uses an explicit bounded
turn-local preview-registry handle, and the handle is retained by the registry
for a future sibling adapter without global state or downcasting. Transport
LSP preview apply now resolves its nested session locator through the normal
project-scoped `file.modify` authorization path. The daemon handler reuses one
fail-closed session/workspace binding helper before invoking the existing
checked mutation owner. No model mutation tool was registered in M006.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Daemon LSP service is threaded into session tool construction | `TurnRunInput.lsp_service` is passed into `SessionToolContext`, then `ToolRegistryOptions`; `tests/tool_registry.rs` covers the session factory identity | Pass |
| Model `lsp` and hidden `lsp_read` use the supplied service | `ToolRegistry::with_options` creates one `lsp_service_shared` and clones it into both registrations; registry service identity and factory tests cover the supplied path | Pass |
| Preview state is explicitly shared and turn-local | `LspPreviewRegistryHandle` is `Arc<Mutex<PreviewArtifactRegistry>>`; `LspTool` accepts it; registry retains it; `src/tool/lsp.rs` tests prove shared-handle visibility and default isolation | Pass |
| Transport apply resolves session-owned project scope | `operation_descriptor` maps `lsp_preview_apply` to `ViaSession` + `FileModify`; `session_id_for_request` extracts `request.session_id`; matrix tests and authorization guard pass | Pass |
| Session/workspace binding is reusable and fail closed | `src/lsp/mutation.rs::validate_session_workspace_binding` validates typed identities and distinguishes invalid, missing, mismatch, and storage failures; daemon apply calls it | Pass |
| Existing checked mutation remains the sole owner | `src/lsp/mutation.rs::apply_preview` remains the only apply implementation; the daemon still delegates to it and no model mutation adapter was added | Pass |
| M005 construction seam exists | Registry retains the LSP service identity and preview handle alongside the existing workspace/session runtime construction; factory regression test proves the seam | Pass |

## 3. Production implementation evidence

The runtime trace is:

```text
TurnRunInput
  ├─ Arc<LspService> ─┐
  ├─ ExecutionContext │
  └─ workspace lease   │
          ↓            │
SessionToolContext     │
          ↓            │
ToolRegistryOptions ───┘
  ├─ one LspService Arc shared by lsp and lsp_read
  └─ one LspPreviewRegistryHandle retained by ToolRegistry and LspTool
```

`ToolRegistry::lsp_service_identity()` and the session-factory test provide
Arc identity evidence. The shared preview test registers through one explicit
handle and observes the entry through `LspTool`; separately constructed default
tools remain empty. The registry handle is neither serialized nor persisted,
and dropping the registry drops its turn-local ownership.

The authorization trajectory is:

```text
CoreRequest::LspPreviewApply
  → ViaSession + file.modify
  → nested request.session_id → owning project
  → session/workspace binding helper
  → canonical apply_preview
```

## 4. Verification executed

All commands below were run locally at the implementation baseline, with the
final broad checks run after the implementation commit:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo test --test tool_registry` | passed |
| `cargo test -p codegg --lib lsp::mutation` | passed |
| `cargo test -p codegg --lib tool::lsp` | passed |
| `cargo test -p codegg-core authorization` | passed |
| `cargo test --test lsp` | passed |
| `cargo test --test identity_m003_daemon_authorization` | passed |
| `cargo test --test presence_m003_observation` | passed |
| `python3 scripts/check_authorization_matrix.py` | passed |
| `python3 scripts/check_execution_ownership.py` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `scripts/verify.sh quick` | passed |
| `git diff --check` | passed |

An initial quick-gate attempt caught a missing `lsp_preview_registry` field in
the production factory initializer. That compile-only defect was fixed before
the final verification above; no failing behavioral result was ignored.

## 5. Invariant review

- `apply_preview` remains the sole checked LSP preview mutation owner.
- Preview-producing LSP operations remain read-only.
- Preview registries are bounded, explicit, ephemeral, and isolated per
  registry/turn; no daemon-global or persisted registry was introduced.
- Model input does not gain workspace, session, patch, digest, or hash
  authority through M006.
- The canonical workspace lock table remains owned by the workspace service
  lease and is not duplicated.
- Tool Programs receive no LSP mutation path.
- Local-owner compatibility and fail-closed team scope behavior remain under
  the existing authorization service.

## 6. Failure and recovery review

M006 adds no mutation execution path. Missing LSP service continues to use the
existing explicit fallback/disabled behavior; no process-global service is
consulted. Preview state naturally disappears with registry teardown and is
invalidated by restart. Binding lookup errors, missing rows, malformed
identities, and workspace mismatches fail before mutation. No lock is acquired
by the binding helper.

## 7. Migration and compatibility review

No storage migration or wire DTO change was made. Existing LocalOwner/TUI
preview-apply callers retain their DTO and daemon path. The intentional
authorization change is limited to resolving the existing session locator to
the owning project for `file.modify`; viewers, non-members, revoked members,
unknown sessions, and cross-project bindings remain denied or fail closed.

## 8. Security review

The scope correction narrows team authorization to the session-owned project
instead of relying on opaque LocalOwner-only treatment. The handler still
performs trusted host-side workspace/session binding before checked mutation,
and the mutation service still validates containment, digest, per-file hashes,
locking, rollback, checkpointing, file-change projection, and LSP sync.
No second authorization owner, identity source, mutation owner, or persistence
surface was added.

## 9. Documentation and operations

Updated `architecture/lsp.md`, `architecture/tool.md`, and
`architecture/authorization.md` with service reuse, preview lifetime, factory
composition, and `via_session` authorization semantics. The repository quick
gate and all-features Clippy remain the operational verification posture.

## 10. Unresolved findings (severity: critical/high/medium/low)

None.

## 11. Roadmap disposition

M006 is closed. M005 now has all hard dependencies satisfied: M001 is closed,
ADR-0008 is accepted, and M006 is closed with the required runtime, preview,
authorization, and binding seams. M005 is therefore dependency-ready and may
consume the shared host seam. M007 is independently dependency-ready and
remains ready for its evidence-reconciliation pass.

## 12. Registry updates

The M006 implementation plan and roadmap are marked closed and this record is
the accepted closure evidence. M005 is moved from `blocked` to `ready` in the
same closure commit. The M007 row remains `ready`; it has no dependency on
M006 and was not silently changed. The unrelated blocked plans in the registry
retain their existing blockers. This is the explicit unblock audit required by
the planning process.
