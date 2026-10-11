# ADR-0015: Project Initialization Direct-Scope Publish

Status: accepted

Date: 2026-10-10

Decision owners: project maintainers

Related ADR:

- `plans/adrs/ADR-0014-project-initialization-publication.md` (superseded)

Affected subsystem roadmap:

- `plans/subsystems/repository-initialization-roadmap.md`

## Context

ADR-0014 selected a token-only publish request. Repository evidence showed
that CodeGG's authorization descriptor uses `DirectProject` scope, which
requires the request to carry a project locator before dispatch. Resolving
that locator from token state in the authorization preamble would create a
second scope-resolution mechanism and would make auditing less explicit.

## Decision drivers

- Keep the existing exhaustive authorization descriptor as the single
  classifier for operation scope and capability.
- Keep the target path and candidate content out of the publish request.
- Bind approval to the authenticated client and to the exact project and
  workspace shown in the preview.

## Decision

Supersede ADR-0014's token-only publish request detail. The publish request
carries the one-use draft token plus the project and workspace IDs. The
daemon's existing authorization preamble classifies publication as a
direct-project `file.modify` operation. The handler consumes a token only
when its stored client, project, workspace, canonical root, and expected
target digest match the authorized request. The request still accepts no
client-provided path or replacement content; the only target remains the
workspace-root `AGENTS.md` captured in the draft.

## Consequences

### Positive

- Authorization and audit scope remain visible in the typed request and
  continue through the existing direct-project gate.
- The one-use token remains the sole source of candidate content and
  observed target state.

### Negative

- The frontend and daemon protocol carry project and workspace IDs in both
  draft and publish operations.

### Neutral or deferred

- No persistent draft record, storage migration, or arbitrary-path mutation
  API is introduced.

## Compatibility and migration

There is no stored-data migration. Older peers that do not know these typed
variants cannot run `/init`; the TUI surfaces the failed Core request. Existing
`DocumentSave` and LSP mutation contracts are unchanged.

## Security and reliability implications

A token copied across clients, projects, or workspaces fails scope validation.
The token is consumed before target mutation begins, so retries after an
ambiguous response must create a fresh preview and pass the same digest check.

## Verification

The authorization matrix classifies draft as direct-project `file.read` and
publish as direct-project `file.modify`. Core daemon tests exercise draft,
approved creation, update, and stale-target rejection.

## Supersession

Supersedes `plans/adrs/ADR-0014-project-initialization-publication.md`.
