# Post-Audit Maintainability and Surface M007 — Closure Status

Status: closed

Source implementation plan:

- plans/implementation/post-audit-maintainability-surface/007-mcp-oauth-crypto-key-lifecycle-convergence.md

Source subsystem roadmap:

- plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md#7-milestones

Repository baseline reviewed: 98bc89fa613f5a1390202a90b497f59d5732d431

Implementation commits:

- aacf584 — feat(mcp): converge OAuth token crypto lifecycle
- this closure commit — record M007 evidence and planning disposition

## 1. Executive finding

M007 is strictly closed. New MCP OAuth token-store writes use the canonical
master-key resolver and codegg_providers::crypto v2 encryption. Historical
CODEGG_ENC_v1 files remain recoverable through a decrypt-only compatibility
reader and are migrated only after secure temporary write, canonical
decrypt/read-back, and semantic token-set verification.

The MCP TokenSet lifecycle remains MCP-owned; provider CredentialStore was not
used as a lossy multi-secret container. OAuth endpoints, PKCE/state handling,
callback behavior, token refresh/revocation APIs, and server ownership were
not changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| New writes use canonical key and crypto | serialize_v2_tokens calls get_master_key and encrypt_to_string; envelope is CODEGG_MCP_ENC_v2:v2:... | pass |
| Legacy v1 remains readable | get_legacy_key and decrypt_legacy_v1 retain the historical key normalization and AES-GCM reader only | pass |
| Legacy migration is transactional | Secure 0600 temp file, flush/sync, v2 decrypt/read-back, semantic comparison, atomic replace | pass |
| Failed migration preserves source | Read-only-directory failure and corrupt/unknown-store tests leave original bytes unchanged | pass |
| Missing/wrong keys fail closed | v2 requires canonical master key; v1 requires CODEGG_TOKEN_KEY; plaintext/unknown formats are rejected | pass |
| Mutation policy is explicit | Legacy-only load is usable in memory; persistence requires the canonical master key and never emits new v1 data | pass |
| Replay semantics remain stable | Legacy raw code keys migrate to SHA-256 digests; lookup, insertion, removal, and expiry use the same digest | pass |
| Secret-safe diagnostics/debug output | Structural errors and redacted TokenSet/ServerTokens debug implementations; no token/key values in tests | pass |
| Provider credential storage remains unchanged | No CredentialStore schema or provider persistence changes | pass |
| Documentation is current | architecture/mcp.md, architecture/crypto.md, architecture/auth.md, and architecture/config.md updated | pass |

### Non-secret format and migration matrix

| Store | Keys present | Expected behavior | Evidence |
|---|---|---|---|
| v2 envelope | canonical master key | Load normally; no legacy key required | v2 restart test |
| v2 envelope | no canonical master key | Fail closed; preserve bytes | missing-key test |
| v2 envelope | wrong canonical key | Fail closed; preserve bytes | wrong-key test |
| v1 envelope | legacy key only | Load in memory; retain source; mutations require canonical key | legacy-only test |
| v1 envelope | legacy + canonical key | Load, verify, and atomically migrate to v2 | migration/restart test |
| v1 envelope | canonical key only | Cannot decrypt; preserve source | decoder key separation |
| corrupt/unknown/plaintext | any | Reject without overwrite | corruption/plaintext test |
| used-code legacy map | none | Hash live entries, preserve expiry/replay equality, atomically version the file | digest migration test |
| used-code v1 envelope | none | Load digest keys and continue expiry cleanup | versioned loader |

## 3. Production implementation evidence

src/mcp/auth.rs now has one new-write path:

1. Serialize the MCP-owned Vec<ServerTokens>.
2. Resolve the canonical master key through
   codegg_config::encryption::get_master_key().
3. Encrypt through codegg_providers::crypto::encrypt_to_string.
4. Persist through a unique 0600 temp file, sync_all, and atomic rename.

The former local encryption writer, key truncation/hash writer path, and
plaintext fallback were removed. Local AES-GCM code remains only in
decrypt_legacy_v1 and the test-only synthetic v1 fixture. A source census
found no second v2 AES/KDF/key-normalization implementation.

Used authorization-code persistence is now a versioned JSON envelope with
SHA-256 digest keys. Legacy raw-key files are read once, expiry-filtered,
digest-migrated, and rewritten atomically; all replay comparison paths hash
the candidate code consistently.

## 4. Verification executed

All commands below were run locally. The documented static-LZMA linker
environment was used for root executable/test targets because this host's
x86_64 toolchain otherwise resolves arm64 liblzma.dylib.

    rtk cargo fmt --all -- --check
    rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test -p codegg --lib mcp::auth -- --test-threads=1
    rtk cargo test -p codegg-providers --lib -- --test-threads=1
    rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --test mcp -- --test-threads=1
    rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
    rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 scripts/verify.sh quick
    rtk git diff --check

Results:

- Focused MCP auth tests: 7 passed.
- Provider crate tests: 127 passed.
- Existing MCP integration tests: 26 passed.
- All-feature workspace Clippy with -D warnings: passed.
- scripts/verify.sh quick: passed.
- Formatting and diff checks: passed.
- The first unqualified root test attempt was blocked only by the known host
  linker architecture mismatch; the workaround invocation passed.

## 5. Invariant review

- Access/refresh tokens, authorization codes, master keys, and legacy keys are
  not emitted by the new persistence diagnostics or debug formatting.
- New token writes require the canonical master key and canonical crypto.
- v1 compatibility is read-only and bounded to the MCP token-store decoder.
- Provider credential/config encrypted values are untouched.
- Owner-only permissions are applied to new token and used-code files and to
  migration temp files on Unix.
- Token-store scope remains bounded to MCP server token sets; no generic
  secret-store or dependency-injection framework was introduced.
- OAuth protocol, PKCE, state, callback, refresh, revoke, and server-map
  semantics remain unchanged apart from at-rest representation.

## 6. Failure and recovery review

Migration writes to a unique sibling temp file, uses owner-only permissions,
flushes and syncs it, decrypts and parses it, compares the semantic
ServerTokens content, then atomically replaces the source. Temp files are
removed on failure. The original legacy file remains readable when
encryption, temporary write, read-back, or replacement fails.

Startup retains the existing behavior of logging a structural load failure and
continuing without loaded OAuth tokens. A successfully decrypted legacy store
remains available in memory even when migration is deferred; a later mutation
cannot silently recreate v1 because the canonical master key is required.
No background migration task or new token-write race was introduced.

## 7. Migration and compatibility review

The explicit v2 discriminator is CODEGG_MCP_ENC_v2: followed by the
canonical v2: ciphertext. The legacy discriminator remains exactly
CODEGG_ENC_v1. Unprefixed plaintext is no longer accepted, so an unknown or
plaintext file cannot be mistaken for a valid token store or overwrite a
credential source.

CODEGG_TOKEN_KEY is deprecated and is only used for historical v1 reads.
Canonical key lookup preserves the existing precedence:
CODEGG_MASTER_KEY, CODEGG_ENCRYPTION_KEY, then OPENCODE_ENCRYPTION_KEY.
The v1 reader should be removed only after an explicit supported-version
retention or user-migration policy, because historical encrypted files remain
a compatibility contract.

The used-code format change is bounded and lossless for live entries:
SHA-256 preserves equality and expiry behavior without retaining raw code
strings. Expired entries remain discarded by the existing cleanup rule.

## 8. Security review

The new path has no independent KDF, key truncation, or AES encryption
authority. Canonical Argon2id/AES-GCM implementation and master-key aliases
are reused. Wrong keys, missing keys, corrupted ciphertext, truncated input,
and plaintext all fail closed.

The raw authorization-code exposure was assessed and corrected in scope:
all new and migrated used-code keys are one-way SHA-256 digests, and replay
lookups hash consistently. Test fixtures are synthetic and no fixture value is
included in this closure record.

No symlink-following write was added: replacement targets are replaced by
rename, and no new network, protocol, authorization, or OS-keychain surface
was introduced.

## 9. Documentation and operations

Updated:

- architecture/mcp.md — v2 envelope, legacy migration, key precedence,
  used-code digest format, and mutation behavior.
- architecture/crypto.md — MCP as a canonical-crypto consumer and v1
  decrypt-only compatibility note.
- architecture/auth.md — MCP TokenSet distinction from CredentialStore and
  key lifecycle.
- architecture/config.md — deprecated CODEGG_TOKEN_KEY operator entry.

No CI lane, scanner, migration framework, provider integration, or OAuth
protocol change was added.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| critical/high/medium | None | No corrective pass required |
| low / intentional | Historical v1 files still require CODEGG_TOKEN_KEY until migrated | Documented compatibility contract; retain the reader until an explicit removal policy is accepted |
| low / operational | A host without a canonical master key can read legacy tokens but cannot persist mutations | Deliberate fail-safe behavior; operator guidance is documented |

## 11. Roadmap disposition

M007 is closed, and the post-audit maintainability corrective addendum is
complete because M006 and M007 both have accepted strict closure records.
The configured search/eggsearch compatibility fallback remains independently
closed and is not reopened.

The blocked-work audit found no registered future plan whose hard or interface
dependency is this post-audit M007. The blocked TUI M006/M008/M009/M010 rows
remain blocked on their own TUI M005/TUI M007 contracts, while architecture
and runtime conditional evidence remains independent. No plan was unblocked or
otherwise status-promoted by this closure. Deferred generalized OAuth/provider
credential unification remains intentionally unregistered.

## 12. Registry updates

The closure commit:

- marks the implementation plan implemented — closed and links this record;
- marks the corrective addendum and registry subsystem row closed;
- removes post-audit M007 from dependency-ready/active tracking;
- records M007 under recently closed work with implementation commit aacf584;
- records that no registered downstream plan became ready.

No corrective pass or ADR is required.
