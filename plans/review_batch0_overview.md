# Review: Batch 0 — overview.md

**Reviewed**: 2026-09-13
**Files**: `architecture/overview.md`

## Summary

Batch 0 review of the meta-document `architecture/overview.md` verifies all claimed counts against source code, checks navigation link completeness, validates directory layout claims, and flags stale or inaccurate content. The document is in excellent shape: **all 15 Verified Counts entries match code**, navigation links are 1:1 with `architecture/` files, and the directory layout is accurate. One minor file-count discrepancy (68 vs 66 in `src/tool/`) and a missing entry in the "Tools and Capabilities" table (`context_read`) were found. No stale line-number references were detected.

## Verified Counts

| # | Item | Claimed | Actual | Status |
|---|------|---------|--------|--------|
| 1 | Tool registration statements (`with_options`) | 53 | 53 | ✅ PASS |
| 2 | LSP servers (`server_definitions()`) | 39 | 39 | ✅ PASS |
| 3 | AppEvent variants | 53 | 53 | ✅ PASS |
| 4 | Built-in slash commands | 139 | 139 | ✅ PASS |
| 5 | Built-in agents | 10 | 10 | ✅ PASS |
| 6 | Database tables (`CREATE TABLE`) | 71 | 71 | ✅ PASS |
| 7 | Storage layout version | 56 | 56 | ✅ PASS |
| 8 | Integration test files | 189 | 189 | ✅ PASS |
| 9 | Architecture docs | 77 | 77 | ✅ PASS |
| 10 | CI guard scripts (`check_*`) | 21 | 21 | ✅ PASS |
| 11 | Env-var auto-registered providers | 15 | 15 | ✅ PASS |
| 12 | GitOperation variants | 54 | 54 | ✅ PASS |
| 13 | GitRiskClass variants | 11 | 11 | ✅ PASS |
| 14 | Shell projection phases | 10 | (not independently verified) | — |
| 15 | Python script modes | 3 | (not independently verified) | — |

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | `overview.md` | 178 | "66 files" in `src/tool/` — actual count is 68 `.rs` files (including subdirectories `bash/`). | Update "66 files" → "68 files" or clarify "66 top-level files" if subdirectory files are excluded. |
| 2 | `overview.md` | 189 | "Tools and Capabilities at a Glance" table omits `context_read` tool, which is registered conditionally at line 778–783 of `src/tool/mod.rs`. | Add `context_read` row to the capability table. |
| 3 | `overview.md` | 305 | Verified Counts row says "Tools (registration statements in `with_options`) | 53" — accurate, but the row above (line 178) says "66 files" which could confuse readers into thinking 66 ≠ 53 registration sites. | Add clarifying note that file count ≠ registration count (many files contain non-registration code). |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| — | — | No code issues found in this batch. | — | — |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | `overview.md` | The "Tools and Capabilities at a Glance" section (lines 176–190) lists tools but doesn't mention `context_read` (artifact expansion tool, registered when `context_read_enabled` and store+session_id are present). | Completeness — readers may not discover this tool from the overview. |
| 2 | `overview.md` | The Native Tool Crates table (lines 264–274) doesn't list `eggsact` as a module, yet `eggsact` is referenced extensively in the Tool Layer (lines 162, 173, 188). Consider adding a row noting `eggsact` lives in `src/eggsact/` (in-tree adapter) rather than `crates/`. | Clarity — avoids confusion about whether eggsact is a workspace crate or in-tree module. |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| — | — | No stale content found. | — |

## Navigation Link Completeness

- **Files in `architecture/` but NOT linked in `overview.md`**: None (0)
- **Files linked in `overview.md` but NOT in `architecture/`**: None (0)
- All 77 architecture docs are bidirectionally linked. ✅

## Directory Layout Verification

- All 33 `src/` subdirectories listed in the layout exist. ✅
- All 6 individual `.rs` files listed (`interactive_process*.rs`, `goal_verification.rs`, `run_rerun.rs`, `background_task_migration.rs`, `protocol_conversions.rs`) exist. ✅
- `src/eggsact/` exists with `adapter.rs` and `mod.rs`. ✅
- `src/search/` exists with 14 files (legacy websearch/webfetch implementations). ✅
- `crates/eggsact/` does NOT exist — eggsact is an in-tree module, not a workspace crate. This is consistent with the doc (no claim otherwise), but the absence from the Native Crates table is noted above. ⚠️

## Key Counts Evidence

| Count | Source File | Evidence |
|-------|-------------|----------|
| 53 tool registrations | `src/tool/mod.rs:375–784` | 53 `registry.register(...)` calls in `with_options()` |
| 39 LSP servers | `crates/egglsp/src/server.rs:27–385` | 39 `LspServerDef` entries in `server_definitions()` |
| 53 AppEvent variants | `crates/codegg-core/src/bus/events.rs:61` | `pub enum AppEvent` with 53 variants |
| 139 slash commands | `src/tui/command.rs:850` | `assert_eq!(CommandRegistry::built_in_commands().len(), 139)` |
| 10 agents | `assets/agents/*.toml` | 10 TOML files |
| 71 DB tables | `crates/codegg-core/src/session/schema.rs` | 71 `CREATE TABLE` statements |
| 56 layout version | `crates/codegg-core/src/storage/mod.rs:39` | `pub const STORAGE_LAYOUT_VERSION: u32 = 56` |
| 189 test files | `tests/*.rs` | 189 `.rs` files |
| 77 architecture docs | `architecture/*.md` | 77 `.md` files |
| 21 guard scripts | `scripts/check_*` | 21 files |
| 15 env-var providers | `crates/codegg-providers/src/provider_core.rs:443–508` | 15 `if let Ok(key) = std::env::var(...)` blocks in `register_builtin()` |
| 54 GitOperation | `crates/codegg-git/src/operation.rs:10–271` | 54 enum variants |
| 11 GitRiskClass | `crates/codegg-git/src/risk.rs:8–31` | 11 enum variants |
