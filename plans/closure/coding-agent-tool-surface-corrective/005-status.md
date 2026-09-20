# Coding-Agent Tool Surface Corrective M005 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md`
Source subsystem roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m005--checked-lsp-preview-application`
Repository baseline reviewed: `b31ce65` (implementation commit; final verification ran on this tree)
Implementation commits: `b31ce65` — add checked LSP preview apply adapter; `65d8d9f` — activate M005

## 1. Executive finding

M005 is strictly closed. A session-scoped native `lsp_preview_apply` tool now
accepts only an opaque `preview_id`, resolves the current candidate from the
M006 shared turn-local registry, binds host-owned session/workspace/turn
identity, and delegates to the existing checked mutation owner. It is
direct-only, mutating, non-idempotent, non-retryable, and unavailable to Tool
Programs. Successful mutation is the only path that marks the preview applied.

No second mutation, authorization, scheduler, LSP service, preview store, or
checkpoint owner was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Model supplies preview identity only | `LspPreviewApplyInput` uses `deny_unknown_fields`; public schema has only required `preview_id` and `additionalProperties: false` | Pass |
| Candidate is host-owned | `export_preview_apply_candidate()` reads the shared M006 handle; request construction copies revision, digest, kind, title, provenance, patches, workspace, session, and turn identity on the host | Pass |
| Canonical mutation owner is reused | `LspPreviewApplyTool` calls `src/lsp/mutation.rs::apply_preview`; no patch, hash, digest, lock, write, rollback, checkpoint, or sync implementation is duplicated | Pass |
| Correct tool contract | `ToolCategory::Mutating`; `DirectOnly`; `NonIdempotent`; no broker retry; structured output schema wrapping `LspPreviewApplyResultDto` fields | Pass |
| Runtime seam is explicit | Factory registers the adapter only with the pool, canonical workspace locks, daemon LSP service, shared preview handle, execution workspace, session ID, and turn ID | Pass |
| Preview lifecycle is safe | Unknown, stale, empty, unsupported, and already-applied candidates fail before mutation; `mark_applied` runs only after `apply_preview` succeeds | Pass |
| Profile/discovery authority remains bounded | Tool is deferred, gated by LSP availability, and explicitly denied for plan/explore/research/security-review/verifier profiles; Tool Programs cannot call it | Pass |
| Supported preview kinds | Integration fixture applies rename, formatting, and edit-only code-action candidates through the same adapter | Pass |

## 3. Authority and call trace

Human/protocol and model callers converge only after their caller-specific
authority checks:

```text
Human/TUI:
transport principal
  -> AuthorizationService: ViaSession + file.modify
  -> trusted session/workspace binding
  -> export_preview_apply_candidate
  -> apply_preview

Model:
resolved direct-only tool surface + parent ceiling
  -> ToolBroker permission/contract checks
  -> host-bound session/workspace/turn runtime
  -> shared preview candidate export
  -> trusted session/workspace binding
  -> apply_preview
```

The model cannot provide or override a path, patch, original hash, digest,
revision, provenance, workspace ID, session ID, or turn ID. The model tool does
not synthesize a `CoreRequest` or bypass project/session authorization.

## 4. Success and negative trajectories

The adapter unit/integration coverage includes:

- rename, formatting, and edit-only code-action candidates applied to expected
  file content with the canonical checkpoint/result path;
- stale registry candidate rejected before any write;
- already-applied candidate rejected and never replayed;
- unknown/expired IDs rejected before mutation;
- schema tampering with model-supplied `patches` rejected;
- typed session/workspace binding rejects malformed, missing, and mismatched
  bindings; the adapter also rejects a mismatched execution session/turn;
- read-only and programmatic callers are excluded by the direct-only contract
  and explicit read-only agent profile denials;
- command-bearing and resource-operation preview classes remain outside the
  bounded `PreviewArtifactRegistry`/mutation subset and are not enabled.

The existing canonical mutation tests continue to cover digest mismatch,
stale file hashes, containment, lock-serialized writes, rollback, checkpoint
creation, checked recovery, and synchronization warnings. The existing
authorization matrix continues to cover observer/non-member/cross-project
transport denial. Since the adapter holds the same repository lock and calls
the same service, concurrent duplicate application retains the canonical
at-most-one successful write behavior: a losing call observes applied state or
fails the post-first-write hash revalidation. No automatic retry is configured
for an uncertain side effect.

## 5. Compatibility and persistence review

The change is additive. Existing TUI and protocol DTOs remain unchanged; no
storage migration or protocol version bump was made. Preview state remains
turn-local and ephemeral: restart/teardown drops the registry, so old IDs
return not-found/expired rather than gaining durable persistence. Existing
preview creation remains read-only, and opaque LSP commands and
`workspace/executeCommand` remain denied.

## 6. Documentation and generated surfaces

Updated:

- `architecture/lsp.md` — two explicit apply callers and one checked mutation
  owner;
- `architecture/tool.md` — session-scoped adapter, contract, and registration;
- `architecture/agent-tool-surface.md` — deferred disclosure and profile
  restrictions;
- agent TOML definitions plus generated builtins — explicit read-only profile
  denials;
- disclosure and model filtering — deferred discovery and LSP availability
  gating.

The repository does not contain the historical `scripts/check_builtin_agents.py`
named by the quick-start notes; the canonical generator's `--check` mode ran
and passed, and `scripts/verify.sh quick` passed.

## 7. Verification executed

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tool::lsp_preview_apply -- --test-threads=1` | 3 passed |
| `cargo test -p codegg --lib tool::lsp -- --test-threads=1` | 181 passed |
| `cargo test -p codegg --lib lsp::mutation -- --test-threads=1` | 5 passed |
| `cargo test -p egglsp --lib preview_registry -- --test-threads=1` | 30 passed |
| `cargo test --test lsp -- --test-threads=1` | 164 passed |
| `cargo test --test presence_m003_observation -- --test-threads=1` | 11 passed |
| `cargo test --test tool_execution -- --test-threads=1` | 55 passed |
| `cargo test --test edit_checkpoint` | current target absent; Cargo listed no such target |
| `cargo test --test edit_checkpoint_integration -- --test-threads=1` | 22 passed; exact current equivalent |
| `cargo test -p codegg --lib agent::tool_inspect -- --test-threads=1` | 16 passed |
| `cargo test -p codegg --lib tool::disclosure -- --test-threads=1` | 8 passed |
| `cargo test --test tool_registry -- --test-threads=1` | 14 passed |
| `cargo test --test tool_contract_guards -- --test-threads=1` | 11 passed |
| `python3 scripts/check_execution_ownership.py` | passed |
| `python3 scripts/check_scheduler_bypass.py` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `scripts/verify.sh quick` | passed |
| `git diff --check` | passed |

The exact historical `edit_checkpoint` target was attempted and is absent in
the current repository; the current `edit_checkpoint_integration` target was
run instead. This is a target-name drift, not a product failure.

## 8. Unresolved findings

None. The missing historical helper script and test target are documented
repository-instruction drift with passing current equivalents; neither blocks
the closed implementation or a future coding-agent milestone.

## 9. Roadmap and unblock disposition

M005 is closed and the coding-agent tool-surface corrective campaign satisfies
its completion definition: registration/disclosure/authority remain aligned,
M003/M004 evidence is reconciled, the LSP runtime seam is canonical, and
checked preview edits are available to mutation-capable coding agents without
adding a new owner.

The dependency audit found no future plan in this workstream that was blocked
on M005. M005 is the terminal milestone in the roadmap; there is therefore no
additional coding-agent plan to promote. Unrelated blocked workstreams—such as
the dependency-security M005 external-updater blocker and the conditional
architecture/runtime-safety evidence items—remain unchanged and are not
unblocked by this closure.

## 10. Registry disposition

The M005 implementation plan is marked `implemented`, the roadmap milestone
and subsystem are marked `closed`, the dependency-ready M005 row is removed,
and this record is added to recently closed work. M001-M007 are now all closed
for this corrective campaign. Historical closure records remain immutable.
