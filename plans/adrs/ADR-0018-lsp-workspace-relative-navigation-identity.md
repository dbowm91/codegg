# ADR-0018: Workspace-Relative Identity for LSP Navigation Results

Status: accepted

Date: 2026-10-12

Accepted: 2026-10-12, by the project maintainer, selecting Option A: add an
optional validated workspace-relative target field to existing LSP location
DTOs.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`
- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Related ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted)

## Context

M006-B's `LspRangeDto` preserves only a display path, currently the target
file's basename for definition and reference results. The daemon deliberately
does not emit an absolute URI or guess a project-relative path because an LSP
result can point outside the requesting workspace. M006-C's editor navigation
cannot safely open a target from a basename, and the frontend must not infer
path authority from language-server text.

The current `DocumentOpen` operation already accepts a workspace-relative path
and rechecks containment and `file.read` authorization at the daemon. A
navigation identity can therefore be an untrusted locator that is safe to
display and pass back through that existing authorization boundary.

## Decision drivers

- A navigation result must identify one target relative to the exact workspace
  used for the authorized LSP request.
- Absolute paths, file URIs, paths outside the workspace, and symlink escapes
  must not become frontend navigation targets.
- Existing clients must continue to deserialize older responses and treat a
  missing identity as non-navigable.
- Existing display text and result bounds should remain compatible.
- `DocumentOpen` remains the final authority and revalidates every target.

## Considered options

### A. Add an optional validated workspace-relative target field (recommended)

Add `workspace_relative_path: Option<String>` to LSP location results. The
daemon supplies it only after deriving the URI path beneath the authorized
workspace root and validating canonical containment. Keep the existing display
path unchanged. Older responses omit the field and deserialize as `None`.

Benefits: additive wire shape; safe navigation is possible when the daemon has
proved the identity; outside-workspace results can still be displayed without
being opened.

Costs: one DTO field and daemon-side path normalization need regression
coverage. The TUI still must handle a missing target.

### B. Replace the existing path field with a workspace-relative path

Benefits: one path field with clear semantics.

Costs: changes existing display behavior and makes old/new meanings ambiguous;
absolute or cross-workspace results cannot be represented usefully. Rejected
for compatibility and clarity.

### C. Keep location results display-only and narrow M006-C

Remove navigation actions from M006-C and retain only diagnostics and other
results that do not require a target path.

Benefits: no protocol change; smallest implementation.

Costs: does not deliver the registered navigation capability, requires an
explicit scope amendment, and defers that capability to a later plan.

## Decision

Option A is accepted: add an optional workspace-relative target field to
existing LSP location DTOs. Preserve existing display fields, default a missing
field to non-navigable, and use `DocumentOpen` for final authorization.

## Consequences

- Navigation uses a daemon-derived, bounded workspace-relative locator.
- A target outside the authorized workspace remains display-only.
- An older daemon response lacking the field remains usable but cannot trigger
  navigation.
- The LSP service, `file.read` gate, document owner, and storage remain
  unchanged.

## Compatibility and migration

Option A adds an optional serialized response field with a fail-closed `None`
default. It adds no request, authorization operation, database schema, or
storage migration. Older clients ignore the field; newer clients cannot infer
a navigable path from an older response.

## Security and reliability implications

The field is a locator, never authority. The daemon derives it from the
transport-authorized session workspace and the returned LSP URI. Canonical
containment must reject absolute, outside-root, and symlink-escaped targets.
The frontend treats all response data as untrusted and opens a target only
through `DocumentOpen`, which repeats the daemon's path and authorization
checks. Failure to prove a target identity leaves the result display-only.

## Verification

Test workspace-relative locations, URI encoding, sibling-prefix
confusion, outside-workspace results, symlink escapes, missing/older response
fields, and `DocumentOpen` revalidation. Run the existing authorization and
workspace-containment guards and hosted CI.

## Supersession

None.
