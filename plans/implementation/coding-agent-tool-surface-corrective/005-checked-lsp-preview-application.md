# Coding-Agent Tool Surface Corrective M005 — Checked LSP Preview Application

Status: blocked

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (production baseline)

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#M005--checked-lsp-preview-application`

Hard dependency:

- M001 must close first so the new mutation surface receives correct canonical authority/category/discovery semantics.

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`

Applicable ADRs: none if the implementation remains an adapter from existing LSP preview artifacts into existing checked native edit/checkpoint ownership. Stop if a new durable edit owner is required.

Primary class: capability / invariant

## 1. Objective

Add an explicit checked model action for applying a previously generated LSP edit preview.

Today LSP rename/format/source-action/code-action operations intentionally produce previews and never mutate files. The model must then reconstruct or translate the preview into `apply_patch`/other edits. M005 should allow:

```text
LSP preview generation
        ->
preview_id + affected files/base hashes/edit set
        ->
checked apply_lsp_preview(preview_id)
        ->
permission + workspace/stale validation
        ->
canonical native restorable edit checkpoint
```

The preview is never itself authorization. Application is a separate mutating tool call subject to normal permission/sandbox/parent-ceiling policy.

## 2. Why this milestone is blocked

M001 must first fix category/capability authority and discovery semantics. Otherwise a new mutation tool risks entering the same duplicated/fallback classification problem.

Existing LSP preview, patch utilities, snapshot/edit checkpoint, workspace locks, permission checker, and broker provide the required foundations.

## 3. Current implementation evidence

At the baseline:

- `lsp` preview-producing operations include rename, format, source action, code action, and semantic-check preview paths.
- tool output carries `preview_id` and `PreviewMetadata` including `not_applied`, affected files, edit count, and stale-base indication.
- LSP architecture contains a `PreviewArtifactRegistry`/preview representations and explicitly guarantees composed workflows do not apply previews.
- `ToolBatchExecutor` and snapshot affected-path logic own the native restorable mutation set: `write`, `edit`, `replace`, `apply_patch`.
- workspace locks cover pre-capture -> native execution -> post-capture -> persistence for eligible native mutation batches.
- `apply_patch` already has shared patch utilities and allowed-root checks.

The missing ergonomic bridge is a checked consumer of a preview identity that preserves these owners.

## 4. Invariants that must not regress

- LSP query/preview operations remain read-only and never apply edits implicitly.
- Applying a preview is a separate mutating action requiring current authorization.
- Preview identity cannot widen workspace/project/session authority.
- Stale base content, changed file version/hash, missing preview, expired preview, unsafe path, symlink escape, or unsupported workspace edit fails explicitly before mutation.
- Multi-file application is all-or-nothing from the model's perspective unless the canonical checkpoint/batch machinery already defines a safer transactional contract.
- No LSP `workspace/executeCommand` is enabled as part of this work.
- Command-only code actions remain non-applicable unless converted into a safe existing edit representation; do not execute server commands.
- Native edit history/checkpoint remains the restorable mutation owner.
- Preview artifacts do not become a second durable history store.

## 5. Scope

### In scope

- A mutating `apply_lsp_preview` tool or repository-conventional equivalent.
- Exact preview lookup by opaque ID.
- Session/workspace ownership binding for preview artifacts.
- Base/staleness validation using preview metadata plus current file hashes/content revisions.
- Conversion of supported pure text `WorkspaceEdit` previews into canonical patch/edit operations.
- Precompute complete affected path set.
- Permission, allowed-root, symlink/path, workspace-lock, snapshot/checkpoint integration.
- Structured result with applied files/edit count/checkpoint identity where available.
- Single-file and multi-file pure text edits.
- Preview expiry/lifecycle cleanup if the existing registry already supports it.
- Tests and docs.

### Explicitly out of scope

- Applying raw LSP commands or `workspace/executeCommand`.
- Auto-applying previews without a model/user mutation call.
- New conflict merge engine.
- New edit-history database.
- General refactor engine.
- Cross-workspace/cross-session preview application.
- Applying stale previews with best-effort fuzzy matching.
- Persisting preview artifacts across restart unless already supported by the canonical preview registry.

## 6. Required production changes

### Preview artifact contract

Ensure each applicable preview record has enough host-owned data to validate application without trusting model-supplied fields:

- opaque preview ID;
- session/workspace identity;
- operation kind;
- affected canonical-safe relative paths;
- base content hashes or equivalent version evidence for every affected file;
- normalized pure text edits/workspace edit;
- creation/generation identity as already available;
- applicability flag/reason for command-only/unsupported edits.

Do not accept model-supplied replacement edits alongside a preview ID.

### Checked application tool

Input should be intentionally tiny:

```json
{
  "preview_id": "..."
}
```

Optional explicit expected generation/revision is acceptable if already part of preview semantics; the model must not supply paths or edit bodies to override the artifact.

Application sequence:

1. resolve preview and verify caller/session/workspace ownership;
2. confirm preview is pure text edit and marked not-applied;
3. validate all paths through canonical safe-relative-path/symlink/root rules;
4. recompute base hashes/content versions and reject any stale file;
5. compute full affected-path set;
6. enter existing workspace mutation lock/checkpoint boundary;
7. translate preview to canonical native patch/edit operations;
8. apply all supported edits deterministically;
9. capture post-state/checkpoint through existing owner;
10. mark/consume preview as applied only after successful mutation;
11. return structured result/provenance.

If canonical edit-checkpoint machinery cannot make a multi-file preview safely restorable as one logical batch, limit M005 to the supported atomic/restorable subset and report the rest rather than inventing a transaction layer.

### Permission/category

The new tool is `Mutating`/filesystem-write capability and must obey M001 canonical semantic metadata. It must not be categorized based on the `lsp` preview source's read-only status.

### LSP integration

Existing preview-producing operations should advertise the checked application path when the returned preview is applicable. Command-only previews should state why they cannot be applied.

## 7. Ordered work packages

### Work package A — Preview applicability census

Inventory every preview-producing LSP operation and classify output as:

- pure single-file text edit;
- pure multi-file text edit;
- create/delete/rename resource operation;
- command-only;
- mixed/unsupported.

Acceptance evidence: M005 scope is based on actual artifact shapes, not assumptions.

### Work package B — Host-owned base/ownership metadata

Tighten preview records so stale/ownership checks require no model assertions.

Acceptance evidence: cross-session/workspace lookup fails; base hashes cover every mutable file.

### Work package C — Checked conversion to native edits

Implement deterministic conversion for supported text edits using existing patch utilities/edit machinery.

Acceptance evidence: generated edits produce the same target content as the preview representation and enter canonical affected-path/checkpoint logic.

### Work package D — Atomicity/staleness/lock integration

Hold the existing workspace mutation lock across validation capture/application/post-capture where required by current edit-history semantics.

Acceptance evidence: concurrent file change between preview and apply produces a stale/conflict result and no partial checkpoint/application.

### Work package E — Structured result and lifecycle

Return bounded fields such as `preview_id`, `applied`, `affected_files`, `edit_count`, `checkpoint_id`/turn batch identity if available, and failure reason.

Consume/mark applied previews after success to prevent accidental duplicate application; repeated call should return an explicit already-applied/idempotent result according to chosen contract, not replay edits blindly.

### Work package F — Docs/profile integration

Keep preview generation read-only. Add the apply tool to appropriate mutation-capable coding profiles/discovery after M001; read-only reviewers/verifiers must not receive it.

## 8. Failure, cancellation, restart, and contention semantics

- Missing/expired preview: no mutation.
- Stale any-file base: no mutation.
- Unsafe/cross-root path: no mutation.
- Permission denial: no mutation and preview remains available according to existing lifetime.
- Cancellation before first mutation: no mutation.
- Cancellation/failure after mutation begins must use existing ToolBatchExecutor/checkpoint failure semantics; do not invent rollback claims the current edit engine cannot provide.
- Concurrent independent session mutation is serialized through canonical workspace locks for the checkpointed operation.
- Restart: if preview registry is ephemeral, pre-restart IDs become unavailable and return explicit expiration/not-found. Do not add persistence solely to preserve them.
- Duplicate apply after success must not duplicate edits.

## 9. Compatibility and migration

Additive tool only. No database migration expected.

Existing LSP preview output fields remain compatible. Additional host metadata may remain internal or additive. Existing models can continue manually translating previews into patches.

No change to LSP server protocol.

## 10. Required tests

### Focused unit tests

- preview ownership binding;
- stale hash/version detection;
- path/symlink validation;
- edit ordering and overlap validation;
- pure single/multi-file conversion;
- command-only/mixed preview rejection;
- already-applied behavior.

### Integration tests

- renamePreview -> apply -> files changed as previewed -> checkpoint exists;
- formatPreview -> apply;
- sourceAction/codeAction pure edit -> apply;
- multi-file preview if supported by canonical checkpoint path;
- cross-session/workspace denial;
- read-only parent/tool policy denial.

### Contention/cancellation tests

- mutate file after preview before apply -> stale reject;
- two callers attempt same preview -> at most one successful mutation;
- concurrent unrelated workspace mutation respects lock ordering;
- cancellation at defined pre/post dispatch points preserves truthful outcome.

### Restart/recovery tests

- ephemeral preview invalid after restart if that is current canonical behavior;
- no orphaned “applied” marker without actual checkpointed mutation.

### Security/negative tests

- model cannot override paths/edit text under a valid preview ID;
- path traversal/symlink escape denied;
- LSP command execution never triggered;
- preview from another session/project/workspace denied;
- stale preview never fuzzy-applied.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool::lsp
cargo test -p codegg --lib tool::apply_patch
cargo test -p codegg --lib agent::tool_batch
cargo test --test lsp
cargo test --test tool_execution
cargo test --test edit_checkpoint
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use actual current checkpoint test target names at implementation time.

## 12. Documentation updates

- `architecture/lsp.md`: preview lifecycle and checked application.
- `architecture/tool.md`: new mutating surface and category.
- `architecture/snapshot.md`: only if the existing edit checkpoint contract needs an explicit note for LSP-derived native batches.
- `architecture/agent-tool-surface.md`: exposure/authority classification.

## 13. Acceptance criteria

M005 closes when a supported LSP text-edit preview can be applied by opaque ID through a separate permissioned mutation call; every affected file is ownership/path/base validated; stale/unsafe/cross-session/command-only previews fail without mutation; successful application uses canonical workspace lock/edit-checkpoint ownership; and LSP preview operations themselves remain read-only.

## 14. Stop conditions

Stop if:

- M001 is not closed;
- the current preview registry lacks enough host-owned edit/base data and adding it requires a new durable store;
- multi-file correctness would require a new transaction/rollback engine;
- applying a useful preview requires LSP `workspace/executeCommand`;
- canonical snapshot/checkpoint machinery cannot represent the affected operation class safely;
- implementation would bypass ToolBatchExecutor/native edit authority.

## 15. Closure evidence required

Include:

- preview-operation applicability census;
- exact host-owned preview metadata;
- application sequence/owner diagram;
- stale/cross-session/path/command-only negative results;
- duplicate/concurrent apply result;
- edit-checkpoint evidence for successful cases;
- restart behavior;
- exact verification results;
- residual unsupported preview classes.

## 16. Handoff notes

Treat the preview as immutable proposed edit evidence, not as a command. The important property is checked transfer into CodeGG's existing mutation owner. If a preview class cannot make that transfer safely, leave it preview-only.
