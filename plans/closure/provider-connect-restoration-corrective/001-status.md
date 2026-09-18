# Provider /connect Restoration M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-restoration-corrective/001-first-run-credential-key-bootstrap.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-restoration-corrective-addendum.md#m001--first-run-credential-encryption-bootstrap`

Repository baseline reviewed: `375ee316`

Implementation commits or pull requests:

- `568cae33` — feat(auth): first-run managed master-key bootstrap (provider-connect M001)

## 1. Executive finding

M001 is complete. A clean local install can now persist its first
provider credential (and therefore complete `/connect` once M002/M003
restore the neutral UI) with all three legacy master-key variables
absent: the first protected write atomically bootstraps a
CodeGG-managed 256-bit key, restarts resolve it deterministically, and
explicit `CODEGG_MASTER_KEY` deployments keep precedence without
creating a managed file. Pre-existing encrypted material without a
usable key still fails closed with actionable `MasterKeyMissing`
guidance — no silent key replacement. All required tests and guards
pass; no unresolved findings.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `get_master_key()` stays side-effect free (env chain, then existing managed key) | `crates/codegg-config/src/encryption.rs` `get_master_key`/`master_key_with_source`; test `get_master_key_is_side_effect_free_and_env_first` asserts no file is created by reads | pass | — |
| Typed create-on-write resolver for secret-store writes only | `get_or_create_master_key`, `get_or_create_master_key_for_store`, `get_or_create_master_key_at` + `ManagedMasterKey`/`MasterKeySource`/`MasterKeyError` | pass | Startup/load paths untouched |
| Managed key under canonical user config/state root, never project/workspace/cwd | `managed_key_path()` → `<config_dir>/codegg/master.key`; `CODEGG_MASTER_KEY_FILE` override for isolation only | pass | Path recorded in §3 |
| Unix private perms + atomic create-new + symlink/non-regular/unsafe-perm rejection, fail closed | `create_managed_key_file` (`O_EXCL`, `0o600`, fsync); `read_managed_key_file` validation; tests `create_then_resolve_is_deterministic`, `symlink_and_unsafe_permissions_fail_closed` | pass | — |
| Windows user-private persistence without external command | Single `create_new` write under `%APPDATA%/codegg` inheriting profile ACL; documented in module header | pass | No new dependency, no command |
| ≥256-bit CSPRNG entropy; key never printed/logged/serialized/exported | 32 `rand::random` bytes hex-encoded; redacted `Debug`; tests `key_value_never_appears_in_debug_or_error_output`, `bootstrap_errors_and_key_types_are_secret_free` | pass | — |
| Fresh vs. populated store distinguished; populated + missing key keeps `MasterKeyMissing` | `credential_file_has_encrypted_material` / `mcp_token_file_has_encrypted_material` / `has_default_protected_material` gates; tests `missing_store_with_material_refuses_new_key`, `orphaned_store_refuses_replacement_key`, `populated_token_store_without_key_refuses_replacement_key`, CLI `set_key_with_orphaned_material_refuses_new_key` | pass | — |
| Call-site audit: credential writes, provider auth/config encrypted reads, Eggpool provisioner, MCP OAuth, legacy migration | `CredentialStore::put` → `resolve_write_key`; Eggpool `create_inner` maps `AuthError::MasterKeyMissing`; MCP `serialize_v2_tokens` → `resolve_token_write_key`; all read/decrypt sites stay on `get_master_key`; legacy v1 reader unchanged (decrypt-only) | pass | Details in §3 |
| Reads side-effect free; no startup key creation | `load_tokens_sync`/migration require an already-resolvable key (documented in code); no `get_or_create` in load/startup paths | pass | — |
| Fresh write creates one key; restart decrypts | `fresh_store_put_bootstraps_managed_key_and_restart_decrypts`, CLI `set_key_without_master_key_bootstraps_fresh_store`, MCP `fresh_token_store_bootstraps_managed_key_and_restarts`, Eggpool `clean_profile_provision_bootstraps_managed_key_without_env` | pass | Clean-profile Eggpool run is the missing M002-era fixture |
| Concurrent first writes converge | `concurrent_first_writes_converge_on_one_key` (encryption) + `concurrent_first_puts_converge_on_one_key` (store) | pass | `O_EXCL` loser reads winner's key |
| Explicit `CODEGG_MASTER_KEY` works without creating managed key | `explicit_env_key_writes_without_creating_managed_file` | pass | — |
| MCP OAuth same lifecycle or documented exception | `resolve_token_write_key` shares the lifecycle; migration stays read-only by design (documented at `serialize_v2_tokens`) | pass | — |
| Static/behavioral guard against resolver bypass | `scripts/check_master_key_resolver.py` (3 rules); passes | pass | — |
| Env compatibility + no auto re-encryption | Env chain order preserved; existing ciphertext untouched (no migration code) | pass | — |

## 3. Production implementation evidence

Ownership: `codegg-config::encryption` owns all master-key environment
access and the managed-key file. `codegg-providers` (credential store)
and root `src/mcp/auth.rs` / `src/core/eggpool.rs` consume it through
the typed API; `scripts/check_master_key_resolver.py` pins that
direction.

Managed-key path and permission contract:

- Linux: `~/.config/codegg/master.key` (or `$XDG_CONFIG_HOME`); macOS:
  `~/Library/Application Support/codegg/master.key`; Windows:
  `%APPDATA%\codegg\master.key`. `CODEGG_MASTER_KEY_FILE` overrides
  (isolation/testing).
- Content: 64 hex chars = 32 CSPRNG bytes (256 bits).
- Unix: parent `0o700` when newly created; file `O_EXCL` `0o600` +
  data and parent-dir fsync. Reads reject symlinks, non-regular files,
  and any group/other bits, failing closed without repair.
- Windows: one `create_new` write inheriting the user-profile ACL;
  never world-readable; no external command.

Call-site convergence (behavioral):

- `CredentialStore::put` resolves via `resolve_write_key` (env →
  existing managed → bootstrap iff the store is fresh; default location
  additionally guards the sibling MCP token store).
- `src/core/eggpool.rs::create_inner` dropped its preemptive
  `get_master_key().is_none()` gate and maps the store's
  `AuthError::MasterKeyMissing` to `EggpoolError::MasterKeyMissing`
  (stable `master_key_missing` code retained); daemon/user messages
  updated to restore-the-historical-key guidance.
- `src/mcp/auth.rs::serialize_v2_tokens` resolves via
  `resolve_token_write_key` (same lifecycle); `load_tokens_sync` and
  legacy v1 migration remain read-only and require an existing key.
- `AuthResolver::encrypted_value`, `get_plaintext`, `get_credential`,
  `resolve_provider_credential` legacy decrypt, and
  `CredentialStoreAdapter::resolve` stay on the side-effect-free read
  chain (they now transparently benefit from an existing managed key).

Docs: `architecture/crypto.md` (resolution order, file contract,
bootstrap semantics) and `architecture/auth.md` (store + MCP lifecycle,
guard pointer) updated.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-config
cargo test -p codegg-providers auth
cargo test -p codegg-providers
cargo test -p codegg --lib auth
cargo test -p codegg --lib mcp::auth
cargo test -p codegg --lib core::eggpool
cargo test -p codegg --lib auth::cli
cargo fmt --all -- --check
git diff --check
python3 scripts/check_master_key_resolver.py
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings
scripts/verify.sh quick
```

### Results

All local. `codegg-config`: 83 passed. `codegg-providers auth`
filter: 21 passed. Full `codegg-providers`: 151 passed.
`codegg --lib auth` filter: 53 passed. `mcp::auth`: 9 passed.
`core::eggpool`: 11 passed. `auth::cli`: 9 passed. fmt, diff-check,
resolver guard, both Clippy configurations, and `verify.sh quick`
(incl. fmt/agent-schema/core-boundary/sandbox/execution-ownership/
TUI-authority guards + workspace check) all pass.

Deviation from the plan's §7 command list: Clippy was run without
`--all-features` (plus once with the repo-standard
`server,plugins,lsp-test-support` feature set) per `AGENTS.md`
workspace policy — `--all-features` drags in `lsp-real-server-tests`,
which require installed language servers and are never part of default
sweeps. No live-provider network was used; Eggpool provisioning ran
against the deterministic loopback fake server.

## 5. Invariant review

- No secret in chat buffer/argv/logs/SQLite/protocol: key value only
  touches `encrypt_to_string`/`decrypt_from_string` inputs and the
  `0o600` key file; `Debug`/`Display` redacted; error strings contain
  no key material (asserted by tests).
- Explicit env keys retain precedence and never create files.
- Existing ciphertext is never re-encrypted or replaced by this
  milestone; orphaned stores fail closed.
- No process-startup key creation; no project/workspace/cwd key paths.
- `STORAGE_LAYOUT_VERSION`/SQLite untouched — no migration, as planned.

## 6. Failure and recovery review

- Duplicate/concurrent first writes: converge on one key (`O_EXCL` +
  read-back); store-level concurrency test passes.
- Restart: managed key re-resolves deterministically (tested at all
  three layers: store, MCP tokens, Eggpool credential).
- Partial persistence failure: single small `create_new` write +
  fsync; a torn file reads back as corrupt and fails closed with
  removal guidance rather than being overwritten.
- Orphaned/unsafe/corrupt key file: typed `MasterKeyError` with
  recovery guidance; permissions are never silently repaired.
- Cancellation/probe semantics of Eggpool provisioning unchanged; the
  only altered failure branch is the key check, now delegated to the
  store with identical `MasterKeyMissing` mapping.

## 7. Migration and compatibility review

No SQLite migration. No config-schema change. Env-key deployments are
unaffected (same precedence, same ciphertext compatibility including
legacy HMAC-SHA256 reads). Clean profiles gain automatic bootstrap;
populated profiles see identical failure behavior except with more
actionable messages. The managed-key file is additive under the user
config root. `CODEGG_MASTER_KEY_FILE` is a new path-only override
(safe to log; never a secret).

## 8. Security review

- 256-bit CSPRNG key; Argon2id/AES-GCM envelope unchanged.
- `0o600`/`O_EXCL`/fsync creation; symlink, non-regular, and
  group/other-readable files rejected and never auto-repaired.
- Key value excluded from `Debug`/`Display`/errors/logs/snapshots by
  construction (`ManagedMasterKey` redaction) and by test.
- Env-var test isolation uses the process-global env lock plus
  per-test `CODEGG_MASTER_KEY_FILE` overrides; guards in
  `connection.rs`, `auth/cli.rs`, `mcp/auth.rs`, and `eggpool.rs` tests
  were extended so suites stay hermetic on machines with a real
  `master.key`.
- Static guard pins: single env-access owner, read-only allowlist for
  `get_master_key()`, and `.expose()` boundary.

## 9. Documentation and operations

- `architecture/crypto.md`, `architecture/auth.md` updated.
- Operator recovery: restore the historical `CODEGG_MASTER_KEY`
  (aliases accepted); only when old data is expendable, remove the
  encrypted store files (and, if present with no dependents, the
  corrupt/unsafe `master.key`) and retry.
- Guard: `python3 scripts/check_master_key_resolver.py`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

## 11. Roadmap disposition

Milestone M001 closed. Its hard dependent,
`plans/implementation/provider-connect-restoration-corrective/002-provider-catalog-and-neutral-provisioning.md`
(M002), is unblocked to `ready`; M003 remains `blocked` on M002. No
corrective pass required.

## 12. Registry updates

- `plans/registry.md`: register the corrective subsystem (active) and
  M001 (closed, this record, implementation `568cae33`); move M002
  `blocked` → `ready`; keep M003 `blocked` on M002.
- `plans/subsystems/provider-connect-restoration-corrective-addendum.md`:
  M001 `ready` → closed; M002 `blocked on M001 closure` → `ready`.
- `plans/implementation/provider-connect-restoration-corrective/001-*.md`:
  `active` → `implemented`.
- `plans/implementation/provider-connect-restoration-corrective/002-*.md`:
  `blocked on M001 closure` → `ready`.
