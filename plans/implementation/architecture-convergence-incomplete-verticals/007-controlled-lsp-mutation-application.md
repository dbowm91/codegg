# Architecture Convergence M007 — Controlled LSP Mutation Application

Status: implemented

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Interface dependency:

- M002 process/tool execution ownership convergence must close or expose a stable process/edit integration contract.

Relevant long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.5-locality-by-default`
- `plans/000-long-term-specification.md#4.7-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`
- `architecture/lsp.md`

Primary class: capability

## 1. Objective

Extend the existing safe LSP preview surface so supported semantic mutations can complete a controlled preview -> authorize -> apply path through CodeGG's existing checked edit/history ownership. Initial support must include rename and at least one edit-only code action or workspace edit. Command-only arbitrary server actions remain denied unless they can be mapped into already-authorized CodeGG operations without executing opaque server commands.

The target is:

```text
LSP request
   |
   v
preview WorkspaceEdit
   |
   v
validate workspace/document versions + bounds
   |
   v
normal CodeGG mutation authorization
   |
   v
checked edit/apply service
   |
   v
edit-history checkpoint + projection
   |
   v
LSP document synchronization
```

## 2. Explicit non-goals

M007 must not:

- expose unrestricted `workspace/executeCommand`;
- trust server-provided shell commands;
- bypass checked edit history or workspace ownership;
- apply edits outside the active authorized workspace/project boundary;
- silently apply a preview that no longer matches current document versions;
- redesign the LSP transport/runtime;
- add support for every LSP workspace edit variation before the initial vertical path is proven;
- create a second text-edit engine.

## 3. Current implementation evidence to inspect

Inspect at least:

- `crates/egglsp/` request/preview/compatibility code;
- `architecture/lsp.md`;
- root LSP tool wrappers;
- rename preview and code action preview types;
- command-only action rejection behavior;
- workspace edit normalization/validation;
- root edit/apply-patch tools and checked edit-history service;
- runtime-safety M011-M013 mutation attribution/checkpoint behavior;
- document-open/version synchronization;
- projection/UI surfaces for edit previews and completed mutations.

## 4. Required mutation contract

A previewable LSP mutation must yield a bounded typed edit plan containing:

- workspace identity;
- affected canonical paths/URIs;
- expected document/file versions or content fingerprints where available;
- normalized text edits/file operations supported by CodeGG;
- unsupported operation diagnostics;
- source server/method metadata for audit;
- a digest/revision tying apply to the reviewed preview.

Apply must reject stale or mismatched previews and require a fresh preview rather than best-effort merging.

## 5. Ordered work packages

### WP1 — WorkspaceEdit capability matrix

Classify current LSP `WorkspaceEdit` shapes: text edits, document changes, create/rename/delete file operations, annotations, command-only code actions, and server-specific extensions. Mark initial supported subset and explicit denials.

### WP2 — Normalize preview into CodeGG edit plan

Reuse/extend existing LSP preview types to produce one bounded normalized plan consumable by the checked edit service. Do not make the LSP layer directly write files.

### WP3 — Rename apply vertical

Implement rename preview -> explicit apply request -> checked edit-history mutation -> LSP synchronization. Handle stale document/version, partial unsupported edits, and cancellation fail-closed.

### WP4 — Edit-only code action vertical

Support at least one code-action class whose action contains only a safe `WorkspaceEdit`. Command-only and mixed opaque-command actions remain rejected unless the command is explicitly translated to an existing typed CodeGG operation.

### WP5 — File operation policy

If create/rename/delete file operations are already safely representable in current checked mutation APIs, support a bounded subset. Otherwise reject them explicitly and defer rather than bypassing edit history.

### WP6 — Projection/UI/tool integration

Surface preview digest/revision, affected files, unsupported operations, apply result, and stale-preview diagnostics. Ensure model-facing tools cannot skip preview/authorization by calling an internal apply helper directly.

### WP7 — Documentation

Update `architecture/lsp.md` with supported mutation matrix, security model, stale-preview semantics, and explicit command-only denial policy.

## 6. Storage, protocol, migration, compatibility

No durable schema migration is expected beyond existing edit-history records. If preview handles need short-lived IDs/revisions, they should remain bounded session/workspace state unless restart-safe apply is explicitly required by current architecture.

Protocol/tool schema changes may be additive to expose apply operations and stale-preview diagnostics. Existing preview callers remain compatible.

## 7. Security, contention, cancellation

Apply authority is the intersection of current caller/project/session/tool/workspace policy. LSP server output is untrusted input and grants no authority.

All paths must be canonicalized and containment-checked. Cross-workspace URIs or edits are rejected unless current architecture explicitly authorizes them.

Stale preview, version mismatch, overlapping conflicting edits, unsupported resource operations, or lost workspace lease must fail closed without partial hidden mutation. If partial application cannot be made atomic under current checked edit service, the implementation must either add a safe transactional batch boundary or narrow supported edits.

Cancellation before commit/apply completion must not leave edit-history attribution inconsistent.

## 8. Verification

Focused tests must cover:

- rename preview/apply success;
- edit-only code action success;
- stale document/version rejection;
- unsupported command-only action rejection;
- cross-workspace/path traversal rejection;
- checked edit-history attribution;
- undo/reapply compatibility for applied edits where current edit-history semantics support it;
- cancellation/partial-failure behavior;
- LSP document synchronization after apply.

Then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Real-server smoke tests may be used where existing feature-gated fixtures already exist. Do not add a mandatory broad server matrix.

## 9. Acceptance criteria

M007 is complete only when:

- rename completes preview -> authorize -> apply end-to-end;
- at least one edit-only code-action path applies end-to-end;
- apply goes through canonical checked edit/history ownership;
- stale/mismatched previews fail closed;
- command-only opaque actions remain denied unless explicitly mapped to typed CodeGG authority;
- security/containment/cancellation tests pass;
- documentation states the exact supported subset;
- quick verification passes.

## 10. Stop conditions

Stop if implementation requires unrestricted LSP command execution, bypassing checked mutation history, or broad remote-workspace redesign. Narrow the supported subset instead.

## 11. Closure evidence required

Record:

- implementation commits;
- supported/denied WorkspaceEdit matrix;
- rename and code-action end-to-end evidence;
- stale/containment/security evidence;
- edit-history/undo compatibility evidence;
- protocol/tool changes;
- verification outcomes and deferred LSP operations.
