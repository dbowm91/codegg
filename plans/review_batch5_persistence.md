# Review: batch5 persistence-identity
**Reviewed**: 2026-09-13
**Files**: architecture/session.md, architecture/storage.md, architecture/project_catalog.md, architecture/project_identity_storage.md, architecture/identity.md

## Summary

Systematic review of 5 architecture docs against current source code. Found **4 documentation issues** across 4 files. The docs are in significantly better shape than batch1 — most structural claims (table counts, migration wiring, capability lists, identity validation rules) are accurate. The primary divergence is an incomplete identity newtypes listing. No code bugs were identified.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | identity.md | 17–19 | Doc lists 10 identity newtypes but source defines **13** via `typed_identity!` macro. Missing: `AgentRunGroupId` (`identity.rs:251`), `AgentRunMessageId` (`identity.rs:274`), `ChatMessageId` (`identity.rs:280`). | Add the 3 missing newtypes to the listing. |
| 2 | storage.md | 238–261 | Migration list documents v22–v37, v46–v56 but omits v38–v45 and v50 (which exist in `schema.rs:137–174`). A reader may assume these are the only migrations. | Add a cross-reference note: "See `session/schema.rs` for the complete v1–v56 migration chain." |
| 3 | session.md | 56–98 | Table groupings omit `agent_run_group`, `agent_run_group_member`, `agent_run_journal`, `agent_run_mailbox`, `agent_run_result`, `managed_worktree`, `worktree_lease` tables that exist in schema.rs. These were added in undocumented migrations (v38–v50 range). | Add a catch-all "Agent execution and worktree tables (v38–v50)" group listing the remaining tables. |
| 4 | project_identity_storage.md | 88–89 | Doc states `BindingStatus::RebindRequired` as a status but doesn't explain when a session transitions to this state. Source (`project_storage.rs:707–740`) shows `mark_unbound_sessions` sets this for sessions with `workspace_id IS NULL`. | Add one sentence: "Sessions without a resolvable workspace (`workspace_id IS NULL`) are marked `rebind_required` during catalog reconciliation." |

**Note on line-number claims**: All other line references in session.md, project_catalog.md, and project_identity_storage.md were verified against current source and found accurate (e.g., `ProjectCatalog` at `project_catalog.rs:432`, `MAX_ID_LENGTH = 128` at `identity.rs:19`, `Capability::ALL` with 21 entries at `team.rs:295`).

## Code Issues Found

No code bugs were identified during this documentation review. All structural claims that were verified (71 CREATE TABLE statements, STORAGE_LAYOUT_VERSION = 56, 56 migration functions wired, 21 capabilities, identity validation rules, catalog operations, locator kinds, health status variants) match source exactly.

## Improvement Opportunities

| # | Module | Opportunity |
|---|--------|-------------|
| 1 | identity.md | Update the newtypes list to include all 13 types and add a brief "Evolution" note explaining that new identity types are added via the `typed_identity!` macro as new milestones land. |
| 2 | project_catalog.md | Document the `repository_lineage` cross-module dependency in `conservative_legacy_association()` — the function calls `inspect_repository_lineage` from `repository_lineage.rs` but this isn't mentioned in the doc. |
| 3 | storage.md | The `STORAGE_LAYOUT_VERSION` section documents migrations v37, v46–v49, v51–v56 but the interleaving with session.md's table groupings is confusing. Consider consolidating the migration summary into one canonical location (session.md) with storage.md deferring to it. |
| 4 | session.md | Add a cross-reference note for the `conservative_legacy_association` workflow between project_catalog.md and project_identity_storage.md — the identity_diagnostic table bridges them but the relationship is non-obvious. |

## Stale Content to Prune

- **identity.md lines 17–19**: The identity newtypes listing is missing 3 types added for agent execution (AgentRunGroupId, AgentRunMessageId) and chat collaboration (ChatMessageId). These are active types used in the codebase.
- **storage.md lines 238–261**: The migration list is selective (covers high-level milestones) but lacks a catch-all reference to schema.rs. Risk of further drift as new migrations land.
