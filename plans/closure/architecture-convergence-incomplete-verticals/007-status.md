# Architecture Convergence M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/007-controlled-lsp-mutation-application.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `dd03360`

Implementation commit:

- `dd03360` — apply reviewed LSP previews through the checked mutation boundary

## 1. Executive finding

M007 is implemented and closed. Rename, formatting, and edit-only code-action
previews now carry bounded patches plus a revision/digest, and the explicit
`/lsp-preview-apply` command sends that reviewed request to the daemon-owned
mutation service. The service validates workspace/session identity, canonical
path containment, the reviewed digest, and every original file hash; acquires
the existing workspace edit lock; applies the batch with rollback protection;
persists one checked edit-history checkpoint; publishes file-change events;
and synchronizes LSP documents after commit.

The model-facing LSP tool remains read-only. Raw commands, command-only code
actions, and mixed edit-plus-command actions are denied, and resource file
operations remain explicitly deferred rather than bypassing checked edit
history.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Bounded typed mutation contract | `LspPreviewApplyRequestDto` / `LspPreviewApplyResultDto`; preview revision and digest in `PreviewArtifactEntry` | pass |
| Rename preview -> explicit apply -> checked history | `src/lsp/mutation.rs`; `/lsp-preview-apply`; checkpoint undo test | pass |
| Edit-only code-action apply | Shared apply service accepts `code_action`; focused mutation test; existing preview normalizer supplies edit patches | pass |
| Stale/mismatched preview rejection | Digest verification and per-file SHA-256 revalidation; stale test leaves changed content untouched | pass |
| Workspace containment | `validate_path` under the canonical workspace root; cross-workspace test | pass |
| Atomic/rollback behavior | Bounded batch planning, atomic replacement, reverse rollback on write/post-state/checkpoint failure | pass |
| Edit-history attribution and recovery | `EditCheckpointManager::persist_checkpoint`, existing checked undo/reapply path | pass |
| Projection and LSP synchronization | `FileChanged` publication followed by `LspService::update_file`; bounded sync warnings in result | pass |
| Opaque command denial | Raw and command-bearing actions rejected before preview; mixed-action regression test; no execute-command path | pass |
| WorkspaceEdit subset policy | Text edits supported; create/rename/delete resource operations rejected by preview normalization and documented | pass |
| Tool/UI authorization boundary | Only explicit TUI apply command creates the protocol request; model-facing `LspTool` is read-only and refuses applied previews | pass |

## 3. Supported and denied WorkspaceEdit matrix

| Shape | M007 disposition |
|---|---|
| `changes` text edits | Supported when normalized into bounded UTF-8 patches. |
| `documentChanges` `TextDocumentEdit` text edits | Supported through the existing normalizer with current content fingerprints revalidated before apply. |
| Create/rename/delete resource operations | Denied/deferred; no resource mutation bypass was added. |
| Change annotations/server-specific metadata | Never treated as authority; unsupported edit shapes fail closed. |
| Raw `Command` and `workspace/executeCommand` | Denied/not exposed. |
| `CodeAction.command`, including mixed edit-plus-command actions | Denied unless a future milestone maps it to a typed CodeGG operation. |

## 4. Production implementation evidence

- Added additive protocol DTOs and a `CoreRequest::LspPreviewApply` /
  `CoreResponse::LspPreviewApplyResult` transport path.
- Added preview revision/digest derivation and surfaced both in TUI preview
  summaries and the apply request.
- Added the daemon mutation boundary in `src/lsp/mutation.rs`, reusing
  `WorkspaceLockTable`, `EditCheckpointManager`, `validate_path`, the existing
  unified-diff utility, the global file-change projection, and the existing
  LSP document update service.
- Replaced the old TUI-local direct writer from the normal apply command with
  the daemon request/completion path.
- Tightened code-action selection so an opaque command cannot hide behind a
  valid edit payload.
- Updated `architecture/lsp.md` with the supported subset, security model,
  stale semantics, synchronization behavior, and denial policy.

## 5. Verification executed

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass after formatting |
| `cargo check -p codegg-protocol -p codegg --all-targets` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass |
| `cargo test -p codegg --lib lsp::mutation::tests -- --test-threads=1` | test code compiled; test binary link blocked by host toolchain |
| `cargo test -p egglsp --lib operations::code_actions::tests -- --test-threads=1` | test code compiled; test binary link blocked by host toolchain |

The two focused test binaries fail only at final linking because this x86_64
macOS toolchain is given ARM64 `/opt/local/lib/liblzma.dylib` (the linker
reports unresolved `_lzma_*` symbols for x86_64). The repository already
records this host architecture/library mismatch as an operational limitation;
the changed code has no compile or Clippy errors, and quick verification
passes. The failure does not weaken the fail-closed implementation or add a
production correctness finding.

## 6. Failure, cancellation, and security review

- Every affected file is planned and hash-checked before the first write.
- Atomic replacement plus reverse rollback covers write, post-state capture,
  and checkpoint-persistence failures; no checkpoint is persisted for a
  failed batch.
- The workspace lock serializes competing checked edits. Cancellation before
  the durable commit returns through the request future without detaching a
  mutation task; post-commit LSP synchronization errors are warnings and do
  not trigger a second mutation.
- Canonicalization and containment reject traversal, symlink escapes, and
  cross-workspace paths. The daemon resolves the session's workspace before
  invoking the service.
- No server command is executed, no remote workspace authority is inferred,
  and no new durable schema or secret-bearing state was introduced.

## 7. Deferred operations and findings

Resource create/rename/delete operations, server-specific edit extensions,
and opaque command actions remain intentionally deferred. They require a
future typed authority mapping or checked resource-mutation boundary and are
not blockers for this initial text-edit vertical.

No M007-scoped critical, high, medium, or low production correctness finding
remains. The only verification limitation is the local x86_64/ARM64 native
library link mismatch described above.

## 8. Roadmap and dependency disposition

M007's only dependency was M002's stable process/edit integration boundary,
which was already closed. The blocked-work audit found no registered plan
whose blocker is resolved by M007:

- M008 remains `ready`; it is independently dependent only on the closed
  session-projection contract and does not need a status change beyond
  remaining dependency-ready.
- The architecture-convergence roadmap remains `active` because M008 is
  still outstanding.
- The unrelated runtime-safety C002 supported-Linux evidence condition
  remains blocked.

No ADR, migration, new CI lane, or corrective follow-up plan is required.
