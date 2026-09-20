# Coding-agent tool surface corrective M003 closure

Status: closed

## Source and reviewed baseline

- Source plan: `plans/implementation/coding-agent-tool-surface-corrective/003-compact-discovery-and-multiplexed-tool-ergonomics.md`
- Roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md`
- Reviewed implementation baseline: `94cdaaf` (`feat(tool-surface): add compact discovery and git query facade`)
- Closure/status commit: recorded by the commit that updates this record and the registry.

## Executive result

M003 is complete. Broad `tool_search` results are compact descriptors and no
longer carry full input schemas. A policy-checked exact expansion returns the
live canonical schema only after a tool has been selected. The schema census
justified one additive, deferred `git_query` read facade; LSP and task remain
canonical multiplexed surfaces because additional wrappers did not justify
their compatibility and authority cost.

## Work-package matrix

| Work package | Result | Evidence |
|---|---|---|
| A — schema/token census | complete | `tool::tool_search::tests::schema_census_records_large_surface_and_compact_selection` records `git` 5,296 bytes, `git_query` 709, `lsp` 7,405, `task` 3,936; the LSP search payload fell from 8,484 to 1,515 bytes (83%). |
| B — compact discovery | complete | Broad results omit `parameters`; exact `{query,name,detail:"schema"}` returns one current schema. |
| C — LSP facade experiment | dispositioned | Existing `lsp` remains canonical; compact discovery removes selection bloat without duplicating LSP operation routing. |
| D — Git facade experiment | complete | `git_query` delegates to `GitReadTool` and exposes only status/diff/log/branches. |
| E — task advanced-surface deferral | dispositioned | `task` remains the canonical durable run-control owner; no second task adapter was justified by the bounded census. |
| F — compatibility/profile/docs | complete | Registry/disclosure, capability mapping, architecture docs, and focused tests updated. |

## Discovery trajectory and authority

The supported trajectory is: broad search → compact candidate selection →
exact name/schema expansion → ordinary broker invocation. Exact expansion
uses the live `ToolCatalog`, the current available-tool allow-list, and the
hidden-tool filter. It cannot describe denied or hidden tools, and it does
not invoke a backend.

`git_query` is a read-only, direct-only, deferred model facade over the
existing Git reader. Git mutation, recovery, network, and raw compatibility
operations remain on `git`. No LSP or task execution/recovery implementation
was copied, and no new authority owner was introduced.

## Profile and compatibility disposition

The native coding profiles retain their existing canonical `git`, `lsp`, and
`task` compatibility names. `git_query` is registered additively and deferred
so it can be selected for bounded repository inspection without expanding the
initial model surface. Hidden `git_read` and `lsp_read` remain hidden and
program-only. Provider wire names and canonical aliases remain unchanged.

## Verification

- `cargo test -p codegg --lib tool::tool_search`: 3 passed.
- `cargo test -p codegg --lib tool::lsp`: 177 passed.
- `cargo test -p codegg --lib tool::git`: 13 passed.
- `cargo test -p codegg --lib tool::task`: 4 passed.
- `cargo test --test tool_surface_minimization -- --test-threads=1`: 13 passed.
- `cargo test --test tool_program_git_lsp_palette -- --test-threads=1`: 32 passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

The temporary census failure used only to expose the deterministic byte
figures was removed; the permanent census test asserts the compact payload is
smaller than the schema-bearing payload.

## Security and failure evidence

- Hidden and denied exact schema descriptions return `no_results`.
- Broad discovery remains capped at `MAX_SEARCH_RESULTS`.
- `git_query` has a strict four-operation schema, read-only category, direct
  caller policy, disabled cache, and zero retries.
- All calls still use the existing broker, permission, workspace-root, Git
  service, and structured provenance paths.

## Residual findings and unblock audit

No M003 residual blocks remain. M004 (structured verification facade) and M005
(checked LSP preview adapter) are dependency-ready: M004 can use the existing
command-intent/scheduler owners, and M005 can use the already-closed canonical
LSP preview mutation service. Their registry statuses are updated accordingly.
