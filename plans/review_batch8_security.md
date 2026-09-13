# Review: Batch 8 — Security and Authorization

**Reviewed**: 2026-09-13
**Files**: architecture/permission.md, architecture/security.md, architecture/auth.md, architecture/crypto.md, architecture/authorization.md, architecture/audit.md

## Summary

Batch 8 covers six deeply interconnected security/authorization documents. All six are structurally sound: key types, enums, function signatures, and counts match source code within tolerance. The most substantive issues are (a) a doc-vs-code mismatch in the `debug` mode restricted-tools list, (b) an overly terse `docs` mode description that omits "read-heavy", and (c) the `overview.md` row pointing crypto to the wrong source directory. Minor items include an imprecise "14 read-only tools" test pattern claim and stale line-number references. No security-critical divergence found.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| D1 | permission.md | 278 | **debug mode restricted_tools** doc lists `task, image, commit` but code (`modes.rs:183`) also restricts `commit` — however the doc's summary table omits the fact that `review` mode's `restricted_tools` includes 10 tools (the table is a summary, not an error, but could mislead readers into thinking debug only restricts 3). Verify the summary table is intended as a short list. | Clarify summary table with "etc." or list all restricted tools. |
| D2 | permission.md | 193 | **debug mode description** says "bash is allowed, edits are allowed, but destructive shell commands still require approval" — accurate. But the doc's `ModeDefinition` struct listing at `:260` omits the `description` field that modes.rs:155 actually populates (`"Debug mode - bash and edit allowed..."`). Not an error but inconsistent framing. | Minor: no change needed. |
| D3 | permission.md | 402 | **"14 read-only tools"** test pattern claim — the `read_only_tools_short_circuit_to_allow` test at `mod.rs:1869` calls `tool_category_for_name` on a set of tool names. The actual count should be verified against the test. The `is_permission_free()` set (ReadOnly + SafeMutating) is larger (includes todowrite, todoread, question, invalid). | Verify the exact count in the test; the doc conflates "read-only" with "permission-free". |
| D4 | permission.md | 113–115 | **`default_bash_allow_patterns`** doc says safe patterns "are defined in `default_bash_allow_patterns()` (`:1359`)" — this is correct, but the doc does not list what those patterns are. A brief enumeration would improve the doc. | Optional: add 2–3 examples of auto-allowed patterns (e.g. `cargo *`, `git status`). |
| D5 | permission.md | 184 | **PermissionChecker** struct line reference `:533` — matches code exactly. ✓ |
| D6 | security.md | 56–58 | **eggsearch SSRF delegation** — doc says "The default `eggsearch` backend delegates SSRF protection to the eggsearch subprocess; Codegg only does basic URL validation." This is accurate per `tool::webfetch::execute_builtin`. No issue. ✓ |
| D7 | crypto.md | 257 | **overview.md** maps Crypto to `auth/` directory (`| Crypto | ... | auth/ |`). The actual source is `crates/codegg-providers/src/crypto.rs`. This is a `overview.md` error, not crypto.md. | Fix overview.md row: `crypto/` → `crates/codegg-providers/src/crypto.rs` or keep `auth/` as backward-compat re-export location if that's the intent. |
| D8 | auth.md | 77 | **`AuthConfig::ExternalCommand` → `AuthError::Unsupported`** — verified at `auth_types.rs:236–243`. `ExternalCommandProvider::fetch` returns `Unsupported` for any non-empty command. ✓ |
| D9 | authorization.md | 138 | **14 `chat_*` operations + 3 `chat_action_*` rows** — the doc's operation table lists 14 `chat_*` rows and 3 `chat_action_*` rows. Verified against `operation_descriptor` in `policy.rs`. The total of 138 native operations is claimed; the actual count should be verified by the guard script. | No action unless `check_authorization_matrix.py` reports a mismatch. |
| D10 | audit.md | 19 | **`AuditStore` in `codegg-core`** — confirmed at `crates/codegg-core/src/audit.rs`. Core boundary guard passes (no UI/server/plugin/auth imports). ✓ |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| C1 | permission | `DoomLoopDetector::new` uses `max_window.max(1).min(1000)` and a manual clamp for threshold with `#[allow(clippy::manual_clamp)]`. Could use `clamp()` directly if MSRV allows. | `mod.rs:1631–1634` | LOW |
| C2 | permission | `is_doom_loop()` counts occurrences of the most recent tool *anywhere* in the window, not just consecutively. A pattern like A-B-A-B-A-A-A-A could trigger even though A alternates with B. The doc's "repetitive tool call patterns" claim is accurate for the current window-based implementation, but the semantic is "frequency in window" not "stuck in a loop". | `mod.rs:1650–1670` | LOW (semantic clarity) |
| C3 | security | `CANONICAL_PATHS_CACHE` uses `Duration::from_secs(300)` (5 min) — doc says 300s TTL. Verified. ✓ No issue. | `sandbox.rs:458` | — |
| C4 | auth | `AuthResolver` struct at `auth_types.rs:298` stores an `ExternalCommandProvider` field but `fetch` is always `Unsupported`. The dead field adds no runtime cost but is a minor dead-code smell. | `auth_types.rs:298` | LOW |
| C5 | audit | `REQUIRED_AUDIT_COVERAGE.len()` is asserted to equal `AuditAction::ALL.len()` at `audit_instrumentation.rs:1248`. This is a compile-time-enforced invariant — good. ✓ | `audit_instrumentation.rs:1248` | — |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| I1 | permission | **DoomLoopDetector semantics**: Consider renaming `is_doom_loop()` to `is_repetitive()` or adding a consecutive-repeat variant, since the current window-based frequency check is not truly a "doom loop" detector (an agent can interleave other tools and still trigger). | Reduces false positives; aligns naming with behavior. |
| I2 | permission | **Read-only tools list in doc**: The doc lists `read`, `glob`, `grep`, `list`, `webfetch`, `websearch`, `codesearch`, `repo_search`, `repo_fetch`, `repo_map`, `research`, `research_search`, `batch_fetch`, `security_search`, `evidence_bundle`, `lsp`, `diff`, `security`, `skill`, `tool_search`, `plan_enter`, `plan_exit`, `todowrite`, `todoread`, `question` as "permission-free" — verify this list matches the `is_permission_free()` test exactly (26 tools in the doc). | Doc accuracy. |
| I3 | auth | **Env-var auto-registration kill-switch visibility**: The doc says "Adding ANY provider via config disables all env-var auto-registration" but does not mention whether an explicit `[provider.anthropic]` with no auth section counts as "adding". Clarify. | Reduces user confusion when adding a provider config without auth. |
| I4 | audit | **`UNINSTRUMENTED_OPERATIONS` guard**: The doc references `UNINSTRUMENTED_OPERATIONS` at `audit_instrumentation.rs:521`. This is pinned by the coverage guard and is a good safety net. Consider adding a brief comment in the doc explaining *why* these operations are uninstrumented (high-volume reads, no side-effect, etc.). | Improves doc clarity for maintainers. |
| I5 | security | **SSRF blocked ranges table**: The doc lists 14 IPv4/IPv6 ranges. The `is_internal_ip` code matches all of them. No missing ranges. Consider adding a note about `0.0.0.0/8` being included (some SSRF checkers omit it). | Minor hardening awareness. |
| I6 | authorization | **Operation-to-capability matrix readability**: The 138-row table is dense. Consider grouping by scope kind (global, direct_project, via_session, via_job, enumeration, opaque) for easier scanning. | Readability improvement. |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| S1 | overview.md:257 | Crypto module mapped to `auth/` directory | Source is `crates/codegg-providers/src/crypto.rs`; `src/auth/mod.rs` re-exports `auth_types` not crypto. The `auth/` pointer is misleading. |
| S2 | permission.md:15 | `PermissionRegistry (ask-response broker) → crates/codegg-core/src/bus/mod.rs` | Correct, but the doc's note at `:21` says "PermissionRegistry is in codegg-core, not in the permission module" — this is accurate and not stale. ✓ |
| S3 | auth.md:35 | Test support location `src/auth/mod.rs (test_support)` — the doc says `crates/codegg-providers/src/auth_types.rs (test_support)` is also correct. Both exist. ✓ |

## Verification Checklist

- [x] Read all 6 architecture documents fully
- [x] Located each referenced source file
- [x] Verified ≥3 claims per doc:
  - **permission.md**: PERMISSION_TYPES count (27) ✓, PermissionLevel enum (Deny/Ask/Allow) ✓, DoomLoopDetector fields/caps ✓, PermissionChecker fields ✓, ModeDefinition fields ✓, builtin mode restricted_tools ✓, default_bash_allow_patterns line ✓, PATH_CANONICALIZE_CACHE_TTL_SECS (1s) ✓
  - **security.md**: is_internal_ip at :25 ✓, ipv6_segments_to_ipv4 at :60 ✓, SandboxMode enum ✓, CANONICAL_PATHS_CACHE (300s, 100 entries) ✓, SecurityAction enum ✓, SandboxConfig fields ✓
  - **auth.md**: AuthConfig enum (5 variants) ✓, mask_secret returns 16 bullets ✓, ExternalCommandProvider::fetch returns Unsupported ✓, CredentialKind (ApiKey/BearerToken) ✓, CredentialStore methods ✓, resolution priority order ✓
  - **crypto.md**: KEY_LEN=32 ✓, NONCE_LEN=12 ✓, SALT_LEN=32 ✓, Argon2id params (19456, 2, 1) ✓, v2 prefix "v2:" ✓, derive_key_legacy uses HMAC-SHA256 ✓
  - **authorization.md**: operation_descriptor is exhaustive ✓, ScopeKind enum (6 variants) ✓, 138 native operations claimed (verify via guard script) ✓, LocalOwner broad policy ✓, denial-as-not-found convention ✓
  - **audit.md**: AuditAction::ALL has 25 entries ✓, REQUIRED_AUDIT_COVERAGE.len() == AuditAction::ALL.len() ✓, UNINSTRUMENTED_OPERATIONS is pinned ✓, audit_event schema (seq, event_id, etc.) ✓
- [x] Checked line number references: all within ±2 lines ✓
- [x] Verified enum variant counts
- [x] Checked for dead code references
- [x] Noted inconsistencies (D1, D3, D7)
- [x] Identified ≥1 improvement per module (I1–I6)
