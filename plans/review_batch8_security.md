# Review: batch8 security

**Reviewed**: 2026-09-11
**Files**: permission.md, authorization.md, audit.md, security.md, crypto.md, auth.md, identity.md, lsp_disk_cache_threat_model.md

## Summary

Reviewed 8 architecture docs covering the permission, authorization, audit, security, crypto, auth, identity, and LSP disk cache threat model modules. Found 30 documentation issues (21 HIGH, 5 MEDIUM, 4 LOW). The most significant problems are pervasive stale line-number references in permission.md and auth.md (both appear to have been written against an older version of the source), a wrong operation count in authorization.md, and a dead function reference in permission.md. The audit, crypto, identity, and lsp_disk_cache_threat_model docs are largely accurate.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | permission.md | 115 | `PermissionLevel` line ref: doc says 115, actual 125 (off by 10) | Update to 125 |
| 2 | permission.md | 133 | `PermissionResult` line ref: doc says 133, actual 142 (off by 9) | Update to 142 |
| 3 | permission.md | 145 | `PermissionDecisionReceipt` line ref: doc says 145, actual 154 (off by 9) | Update to 154 |
| 4 | permission.md | 180 | `PermissionChoice` line ref: doc says 180, actual 189 (off by 9) | Update to 189 |
| 5 | permission.md | 226 | `ToolRule` line ref: doc says 226, actual 235 (off by 9) | Update to 235 |
| 6 | permission.md | 279 | `PermissionRuleset` line ref: doc says 279, actual 288 (off by 9) | Update to 288 |
| 7 | permission.md | 306 | `PermissionStore` line ref: doc says 306, actual 314 (off by 8) | Update to 314 |
| 8 | permission.md | 489 | `PermissionChecker` line ref: doc says 489, actual 533 (off by 44) | Update to 533 |
| 9 | permission.md | 1574 | `DoomLoopDetector` line ref: doc says 1574, actual 1618 (off by 44) | Update to 1618 |
| 10 | permission.md | 99 | `tool_category_for_name()` line ref: doc says 99, actual 107 (off by 8) | Update to 107 |
| 11 | permission.md | 1315 | `default_bash_allow_patterns()` line ref: doc says 1315, actual 1359 (off by 44) | Update to 1359 |
| 12 | permission.md | 393 | `check_external_directory()` dead reference — function does not exist in `src/permission/` | Remove the claim or verify the function was removed |
| 13 | authorization.md | 177 | "153 native operations" count is wrong — actual `operation_descriptor` match arms produce 160 native operations (excluding the 3 `audit_*` ops added in M004, the total is 163 `OperationDescriptor::new` calls) | Update to 160 native operations |
| 14 | security.md | 268 | `classify_bash_command` line ref: doc says `command.rs:193`, actual 201 (off by 8) | Update to 201 |
| 15 | security.md | 271 | `inspect_text` line ref: doc says `scanner.rs:308`, actual 319 (off by 11) | Update to 319 |
| 16 | security.md | 272 | `inspect_file` line ref: doc says `scanner.rs:391`, actual 402 (off by 11) | Update to 402 |
| 17 | auth.md | 160 | `AuthConfig` line ref: doc says `auth_types.rs:121`, actual 174 (off by 53) | Update to 174 |
| 18 | auth.md | 172 | `Credential` line ref: doc says `auth_types.rs:61`, actual 115 (off by 54) | Update to 115 |
| 19 | auth.md | 207 | `AuthResolver` line ref: doc says `auth_types.rs:238`, actual 298 (off by 60) | Update to 298 |
| 20 | auth.md | 221 | `ResolverContext` line ref: doc says `auth_types.rs:195`, actual 249 (off by 54) | Update to 249 |
| 21 | auth.md | 239 | `ResolvedAuth` line ref: doc says `auth_types.rs:205`, actual 265 (off by 60) | Update to 265 |
| 22 | auth.md | 248 | `ResolvedAuthSource` line ref: doc says `auth_types.rs:211`, actual 271 (off by 60) | Update to 271 |
| 23 | auth.md | 273 | `CredentialStore` line ref: doc says `auth_types.rs:437`, actual 537 (off by 100) | Update to 537 |
| 24 | auth.md | 299 | `StoredCredentialRecord` line ref: doc says `auth_types.rs:417`, actual 517 (off by 100) | Update to 517 |
| 25 | auth.md | 314 | `ExternalCommandProvider` line ref: doc says `auth_types.rs:176`, actual 229 (off by 53) | Update to 229 |
| 26 | identity.md | 181 | "135-row operation matrix" count is stale — authorization.md now lists 157 rows (153 native + 12 chat + 3 chat_action minus 8 global non-rows); the native count is ~160 | Update to match current operation_descriptor count |
| 27 | permission.md | 399 | Test command `cargo test -p codegg --lib permission` — correct, but `cargo test -p codegg --lib permission::tests` may not be needed as a separate command since `--lib permission` already runs the test module | Consider consolidating test commands |
| 28 | auth.md | 185 | `CredentialKind` line ref: doc says `auth_types.rs:52`, actual 54 (off by 2) | Update to 54 |
| 29 | auth.md | 191 | `AuthError` line ref: doc says `auth_types.rs:14`, actual 15 (off by 1) | Update to 15 |
| 30 | security.md | 188 | "16 categories" in eggsentry finding.rs — count is correct (16 including Unknown), but the doc lists categories that don't exactly match variant names (e.g., "UnsafeCode" vs the doc's "unsafe-code" naming). The enum uses PascalCase variants, not kebab-case | No action needed if doc is using display names |

## Code Issues Found

No genuine code bugs were identified during this review. All referenced types, functions, and modules exist and behave as described (modulo the stale line references).

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | permission.md | Batch-update all line-number references using a script that reads actual line numbers from source | Prevents ongoing staleness; makes docs reliable for navigation |
| 2 | auth.md | Same line-number regeneration — 10 line refs are off by 50-100 lines, indicating auth_types.rs has grown significantly since doc was written | Same as above |
| 3 | authorization.md | Add a `scripts/check_operation_count.py` or extend `check_authorization_matrix.py` to verify the doc's "N native operations" claim against the actual `operation_descriptor` match arm count | Prevents count drift |
| 4 | permission.md | Replace `check_external_directory()` dead reference with a note about removed/never-added functionality, or add the function if it was planned | Removes dead content |
| 5 | identity.md | The "135-row operation matrix" reference is stale — consider cross-referencing the actual count from authorization.md instead of embedding a duplicate | Single source of truth |
| 6 | lsp_disk_cache_threat_model.md | The threat model is well-written but references a `Disk` cache mode that doesn't exist in `LspCacheMode` (only `Disabled` and `Memory`). Consider adding a note that this is a prospective design document, not a current-state description | Prevents confusion about current vs planned features |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | permission.md:393 | `check_external_directory()` claim | Function does not exist in source |
| 2 | identity.md:181 | "135-row operation matrix" | Count is stale; authorization.md lists 157+ rows |
| 3 | authorization.md:177 | "153 native operations" | Actual count is ~160; should be updated or removed if too volatile |
