# Post-Audit Maintainability and Surface M007 — MCP OAuth Crypto and Key-Lifecycle Convergence

Status: ready for handoff

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Related canonical implementation:

- `crates/codegg-providers/src/crypto.rs`
- `crates/codegg-providers/src/auth_types.rs::CredentialStore`
- `codegg_config::encryption::get_master_key()`
- `architecture/crypto.md`
- `architecture/auth.md`

Applicable ADRs: none. This plan converges at-rest crypto/key ownership without changing OAuth or provider ownership.

Primary class: invariant / security / maintainability

Closure record to create: `plans/closure/post-audit-maintainability-surface/007-status.md`

## 1. Objective

Move new MCP OAuth token-store encryption onto CodeGG's canonical master-key and cryptographic implementation while preserving the MCP-owned `TokenSet` lifecycle, restart behavior, and a backward-compatible reader for legacy `CODEGG_ENC_v1` stores protected by `CODEGG_TOKEN_KEY`.

The migration must be fail-safe: never overwrite or delete the only readable legacy copy until the canonical replacement has been written and proven readable. Do not force the multi-secret OAuth token-set model into `CredentialStore` unless current APIs can represent it without semantic loss.

## 2. Why this milestone is ready

CodeGG already has a canonical versioned crypto implementation and master-key resolver, while `src/mcp/auth.rs` independently implements AES-GCM key normalization/encryption for token files. The duplication is source-local and can be migrated with an explicit on-disk compatibility reader. No daemon/protocol change is needed.

The provider `CredentialStore` is useful precedent for secure file behavior and canonical key use, but it stores one encrypted secret per provider/account. MCP OAuth token state includes access token, optional refresh token, expiry/type/scope and server URL, so store unification is not a prerequisite for crypto/key convergence.

## 3. Current implementation evidence

Before editing, census:

### MCP OAuth store

- `src/mcp/auth.rs`: `ENCRYPTION_KEY_ENV`, `MAGIC_BYTES`, `get_encryption_key`, `encrypt_data`, `decrypt_data`;
- `TokenSet`, `ServerTokens`, in-memory server map and refresh/revoke/store/remove APIs;
- `save_tokens_async`, synchronous compatibility save/load functions, parent-directory/file creation, temp/rename or permission behavior;
- legacy plaintext handling, if any, and behavior when `CODEGG_TOKEN_KEY` is absent/wrong;
- `mcp_used_codes.json`, its serialized key representation, expiry cleanup, permissions and replay semantics;
- OAuth callback/PKCE/state/replay code flow so persistence changes cannot alter single-use behavior.

### Canonical secret infrastructure

- `codegg_config::encryption::get_master_key()` environment precedence and errors;
- `codegg_providers::crypto::{encrypt_to_string,decrypt_from_string}` v2/legacy behavior;
- `CredentialStore` file write, permissions, atomicity/locking, expiry handling and diagnostics;
- other config/provider migrations using canonical crypto.

Record exact current token-store bytes/formats and filesystem permissions with synthetic fixtures before designing v2.

## 4. Invariants that must not regress

- Access tokens, refresh tokens, authorization codes, master/legacy keys and plaintext decrypted stores never appear in logs, protocol, UI, audit, panic messages or closure evidence.
- New token writes use `get_master_key()` + canonical CodeGG crypto; `src/mcp/auth.rs` no longer implements a second AES/KDF/key-normalization scheme for new data.
- Legacy `CODEGG_ENC_v1` remains readable when the correct `CODEGG_TOKEN_KEY` is available.
- Migration cannot destroy the only readable token store. Failed encrypt/write/fsync/rename/readback leaves the legacy source intact and usable on next start.
- Wrong/missing keys fail closed with actionable structural diagnostics and no fallback to plaintext.
- Token expiry, refresh-token replacement, revocation, replay protection, PKCE/state generation and callback behavior remain semantically unchanged.
- File permissions remain owner-only where supported; symlink/path safety does not regress.
- The MCP token store remains bounded by existing server/token limits and does not become a generic credential database.
- Existing provider credentials/config encrypted values remain untouched.

## 5. Scope

### In scope

- A new versioned MCP token-store encryption envelope using canonical CodeGG master-key/crypto for new writes.
- Legacy v1 decrypt compatibility isolated in a clearly named migration/compatibility module or functions.
- Atomic/read-back-verified migration rules.
- `CODEGG_TOKEN_KEY` deprecation/compat diagnostics that never include key/token values.
- Secure file-write reconciliation with existing credential/config conventions where practical.
- Focused assessment of the replay-used-code store. If raw authorization codes are persisted, determine whether replacing stored keys with a one-way digest preserves exact equality/expiry semantics and can be migrated safely; implement only if bounded and clearly within the same at-rest secret-hygiene boundary.
- Documentation/tests for format/key precedence/migration.

### Explicitly out of scope

- New OAuth provider integrations or OAuth device-flow implementation.
- Changing OAuth endpoints/scopes/PKCE semantics.
- OS keychain integration.
- Redesigning the master-key scheme or Argon2 parameters.
- Moving all `TokenSet` state into `CredentialStore` through lossy encoding.
- General secret-store trait/DI framework.
- Changing provider connection credential storage.
- Persisting OAuth tokens in project configuration.

## 6. Required production changes

### Token-store format

Choose and document an unambiguous version discriminator. Two acceptable patterns are:

1. whole serialized `Vec<ServerTokens>` encrypted with canonical `encrypt_to_string`, wrapped in an MCP store envelope such as `{version: 2, ciphertext: "v2:..."}`; or
2. a text magic prefix identifying MCP store v2 followed by canonical ciphertext.

Prefer a simple explicit envelope that can be recognized without decryption. Do not reuse `CODEGG_ENC_v1` for different crypto.

The serializer owns token-set structure; `codegg_providers::crypto` owns encryption. The canonical master key comes only through `codegg_config::encryption::get_master_key()` for v2 writes.

### Legacy compatibility reader

Move the existing v1 AES-GCM/key-normalization logic into a narrowly named legacy read path. It may read `CODEGG_TOKEN_KEY`; it must not be used for new-format encryption after migration lands. Keep tests pinning current v1 fixtures so future removal has evidence.

### Migration algorithm

At load:

```text
recognize format
  v2 -> require canonical master key -> decrypt/parse
  legacy v1 -> require legacy key -> decrypt/parse
       if canonical master key available:
           serialize/encrypt v2 -> secure temp write -> flush/sync as supported
           read/decrypt/parse temp v2 and compare semantic TokenSet content
           atomically replace original
       else:
           continue in-memory from legacy and emit non-secret migration-needed diagnostic
  unknown/corrupt -> fail closed; do not overwrite
```

If current startup intentionally tolerates token-store load failure and continues without OAuth tokens, preserve that operator behavior while making the reason visible. Do not silently reset a corrupt encrypted file.

### Writes after legacy load

Define explicitly. Preferred: if only legacy key exists and no canonical master key, reads may continue but a mutation requiring persistence fails with an actionable requirement to configure `CODEGG_MASTER_KEY`, leaving the legacy file intact. Do not keep producing new legacy ciphertext indefinitely unless current compatibility requirements prove that necessary.

### Secure persistence

Reuse a small existing secure atomic-write helper if one exists and fits. Otherwise preserve current permissions/temp/rename behavior and strengthen only obvious gaps needed for migration safety. Avoid a repository-wide file-persistence refactor.

### Replay-used-code assessment

Authorization codes are short-lived secrets even after one use. If `mcp_used_codes.json` currently uses raw code strings as map keys, a SHA-256 digest of the high-entropy one-time code can preserve replay equality without retaining the raw code. This is permitted in M007 only if:

- current file schema can be versioned/migrated simply;
- all comparison paths hash consistently;
- expiry cleanup/restart tests remain identical;
- legacy raw entries can be migrated or conservatively discarded only when expired.

If these conditions are not straightforward, document a separate low/medium finding rather than jeopardizing the token migration.

## 7. Ordered work packages

### Work package A — Format/filesystem/key census

Create synthetic legacy/current fixtures and record format discriminator, key resolution, permissions, load-failure behavior, atomicity, and used-code representation.

Acceptance evidence: closure contains a non-secret format/migration matrix.

### Work package B — Canonical v2 serializer/encryption path

Implement v2 encode/decode using canonical master-key + `codegg_providers::crypto`; no local AES/KDF code in the v2 path. Add round-trip/corruption/wrong-key tests.

Acceptance evidence: new writes contain the new discriminator and canonical ciphertext semantics.

### Work package C — Legacy v1 reader and transactional migration

Isolate existing v1 decryption as compatibility-only. Add read -> v2 temp write -> readback semantic equality -> atomic replace. Simulate failures at encryption/write/readback/replace boundaries where test seams exist.

Acceptance evidence: original legacy bytes remain when migration does not complete successfully; successful migration restarts from v2 without legacy key.

### Work package D — Mutation/key transition behavior

Test store/update/remove/refresh after legacy load with and without canonical master key. Provide concise non-secret errors/deprecation diagnostics.

Acceptance evidence: no silent legacy rewrites/new plaintext; mutation behavior is documented.

### Work package E — Used-code at-rest assessment

Inspect whether raw codes are persisted. If bounded digest migration meets section 6 conditions, implement and test it; otherwise record finding with severity and a suggested dedicated follow-up. Do not mix unrelated OAuth changes.

### Work package F — Documentation and removal of duplicate crypto

Delete v2-unused local encrypt/decrypt/key-normalization code, retain only named v1 compatibility logic, and update MCP/crypto/auth docs with key precedence and removal criteria for `CODEGG_TOKEN_KEY`.

## 8. Failure, cancellation, restart, and contention semantics

A migration failure is non-destructive. Existing readable legacy state remains on disk and may be loaded on a later restart with the legacy key. No half-written file becomes authoritative. If atomic replace succeeds but post-replace verification unexpectedly fails, preserve backup/temporary recovery according to the chosen safe-write strategy rather than silently starting empty.

OAuth HTTP request cancellation/timeout behavior is unchanged. Token refresh concurrent with persistence must preserve current serialization/locking semantics; do not introduce a migration task racing normal writes. Migration occurs during bounded store load/init before ordinary mutation or under the existing owner lock.

## 9. Compatibility and migration

Compatibility matrix must cover:

| Store | Keys present | Expected behavior |
|---|---|---|
| v2 | master | load normally |
| v2 | no master | fail/disable OAuth structurally; never plaintext fallback |
| v1 | legacy only | read legacy; no destructive migration; persistence mutation policy explicit |
| v1 | legacy + master | load and transactional migrate to v2 |
| v1 | master only | cannot decrypt legacy; preserve file |
| corrupt/unknown | any | preserve bytes; fail closed/actionable diagnostic |

Define removal condition for the v1 reader: only after documented supported-version retention or explicit user migration policy permits it. Historical encrypted files are a compatibility contract even in pre-1.0 software.

## 10. Required tests

### Focused unit tests

- v2 encode/decode round trip;
- canonical crypto usage and version detection;
- pinned legacy-v1 fixture decrypt;
- wrong/missing key and truncated/corrupt input;
- semantic token-set equality after migration;
- no secret-bearing Debug/error output.

### Integration tests

- OAuthManager restart loads v2 access/refresh/expiry/scope/server state;
- legacy store migrates then restarts without `CODEGG_TOKEN_KEY`;
- token refresh/revoke/remove/store works after migration;
- owner-only permissions where platform allows assertion.

### Restart/recovery tests

- injected temp-write/readback/replace failure leaves original readable;
- unknown/corrupt store is not overwritten on startup;
- used-code replay semantics survive restart if WP-E changes representation.

### Contention/cancellation tests

- concurrent token update/remove follows existing synchronization and cannot interleave migration unsafely;
- no background migration race is added.

### Security/negative tests

- diagnostics contain no synthetic secret/key/token fixture values;
- legacy/master wrong-key cases fail closed;
- symlink/permission behavior does not weaken if existing helpers cover it.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- mcp::auth
cargo test -p codegg-providers --lib -- crypto
cargo test -p codegg-providers --lib -- auth
# focused MCP integration/auth tests if present
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add a network OAuth test or new CI lane; use local/synthetic fixtures.

## 12. Documentation updates

- `architecture/mcp.md`: v2 canonical key/crypto, legacy v1 reader/migration, used-code disposition.
- `architecture/crypto.md`: MCP OAuth as a canonical-crypto consumer and legacy compatibility note.
- `architecture/auth.md`: clarify distinction between provider `CredentialStore` and MCP OAuth token-set persistence if needed.
- user/config docs for `CODEGG_MASTER_KEY` and deprecated `CODEGG_TOKEN_KEY` behavior.

## 13. Acceptance criteria

M007 closes when new MCP OAuth token persistence contains no independent AES/KDF/key normalization, uses the canonical master-key/crypto path, legacy v1 stores remain recoverable and migrate transactionally, failed migration cannot destroy readable credentials, restart/refresh/revocation semantics remain intact, and all documentation/diagnostics are secret-safe.

## 14. Stop conditions

Stop if:

- canonical crypto cannot safely encrypt arbitrary serialized bytes/string token-set payloads without modifying its public contract substantially;
- migration requires deleting/overwriting the only legacy copy before readback verification;
- `CredentialStore` unification would require encoding multiple secrets into one opaque string and becomes the dominant work;
- OAuth protocol/endpoint/provider changes become necessary;
- a generic secret-store framework is proposed;
- current HEAD has already converged new writes onto canonical crypto with tested v1 migration.

## 15. Closure evidence required

Include implementation commits; non-secret legacy/v2 format/key matrix; code search proving v2 uses canonical crypto and local AES/KDF remains compatibility-only or is deleted; successful and failed migration tests; restart/refresh/revoke evidence; permissions and corruption handling; used-code assessment/disposition; documentation updates; focused/broad verification outcomes; and residual findings with severity.

## 16. Handoff notes

Use only synthetic token/key values in tests and never include them in closure output. Prefer whole-store encryption under the existing MCP owner plus canonical crypto rather than forcing a premature generalized credential model. Migration safety is more important than immediately removing the legacy reader.