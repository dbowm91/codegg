# Provider /connect Restoration M001 — First-Run Credential Key Bootstrap

Status: implemented

Closure: plans/closure/provider-connect-restoration-corrective/001-status.md
Implementation: 568cae33

Corrective roadmap:
plans/subsystems/provider-connect-restoration-corrective-addendum.md

Historical closure affected:
plans/closure/provider-connections/002-status.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

## 1. Objective

Remove the manual `CODEGG_MASTER_KEY` prerequisite from a clean local
`/connect` flow without weakening the credential-store boundary or silently
orphaning existing ciphertext.

The result must preserve explicit deployment-managed keys while providing a
CodeGG-managed local key for first-time personal installations.

## 2. Evidence and defect

`codegg_config::encryption::get_master_key()` currently checks only:

1. `CODEGG_MASTER_KEY`;
2. `CODEGG_ENCRYPTION_KEY`;
3. `OPENCODE_ENCRYPTION_KEY`.

`CredentialStore::put` and the Eggpool provisioner return
`MasterKeyMissing` when none exists. The CLI explicitly tells the user to set
`CODEGG_MASTER_KEY`. That contradicts the intended install → `codegg` →
`/connect` first-run path.

M002 verification intentionally tested with a synthetic master-key guard, so it proved
protected storage but did not test a clean environment. Add that missing regression
fixture here.

## 3. Required design

Add a read-only master-key resolver and a lazy create-on-write resolver.

- `get_master_key()` remains side-effect free. It resolves the existing environment
  compatibility chain first, then a CodeGG-managed user-local key when one exists.
- Add a typed `get_or_create_master_key()` (or equivalent service API) used only by
  secret-store write paths. It returns an explicit environment key when configured,
  reuses an existing managed key, or atomically creates a cryptographically random
  managed key for a genuinely fresh local secret store.
- Store the managed key under the canonical CodeGG user config/state root, never the
  project/workspace and never cwd. The filename/path is stable and documented.
- On Unix create the parent/key with private permissions and an atomic
  create-new/write/sync/rename-or-equivalent sequence. Reject symlinks/non-regular
  files and fail closed on unsafe permissions rather than silently repairing an
  attacker-controlled path.
- On Windows use a user-private persistent file/ACL implementation that does not make
  the key world-readable. If the repository lacks a safe platform primitive, add the
  smallest target-specific implementation; do not require an external command.
- The key must have at least 256 bits of CSPRNG entropy and must never be printed,
  logged, serialized into normal config, exported with sessions, or included in
  diagnostics.

Do not generate a new key merely because an existing encrypted store cannot be
decrypted. Before create-on-write, distinguish a truly fresh store from pre-existing
encrypted material. If encrypted credential/OAuth material exists but no usable key is
available, retain `MasterKeyMissing` with recovery guidance. This prevents a new
managed key from making old ciphertext appear permanently corrupt.

## 4. Call-site convergence

Audit all uses of `get_master_key()`, especially:

- `crates/codegg-providers/src/auth_types.rs` credential store writes/reads;
- provider auth/config encrypted values;
- `src/core/eggpool.rs` / the successor generic provisioner;
- MCP OAuth token persistence in `src/mcp/auth.rs`;
- legacy migration code.

Writes that create new protected material should use the create-capable resolver only
when their store is fresh or has already proved the managed key. Reads remain
side-effect free.

Do not make process startup create a key preemptively.

## 5. Compatibility and migration

Environment-provided keys remain supported and retain precedence for deployments that
already use them. Existing ciphertext is not re-encrypted automatically in this
milestone.

If an environment key has historically encrypted a populated store and later
disappears, fail explicitly rather than generating a new key over that store. If a
managed key already exists, restarts resolve it deterministically.

No SQLite migration is expected. If a small local key-source marker is needed to
distinguish store ownership safely, keep it in the user secret/config area and make its
migration additive.

## 6. Tests and guards

Add deterministic tests using isolated HOME/XDG/config roots:

- fresh store + no key env → first credential write creates one managed key and
  succeeds;
- restart + no key env → same credential decrypts;
- two concurrent first writes converge on one key without truncation/race;
- managed key path is private/regular and symlink replacement is rejected;
- populated encrypted store + missing historical key → no new key is generated and
  the error is actionable;
- explicit `CODEGG_MASTER_KEY` continues to work without creating a managed key;
- key value never appears in debug/error/output snapshots;
- MCP OAuth protected writes use the same lifecycle or explicitly document why not.

Preserve existing crypto and auth suites. Add a focused static/behavioral guard so
future credential writes cannot bypass the canonical key resolver.

## 7. Verification

At minimum:

- `cargo test -p codegg-config`
- `cargo test -p codegg-providers auth`
- `cargo test -p codegg --lib auth`
- focused MCP OAuth crypto tests
- provider connection provisioning tests with all master-key env vars removed
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

Use environment serialization/guards in tests; do not introduce order-dependent global
environment races.

## 8. Acceptance / closure evidence

Closure must record:

- the exact managed-key path and permission contract by supported OS;
- proof that no external command/key setup is required for a fresh local credential;
- proof that existing encrypted material never triggers silent key replacement;
- compatibility evidence for explicit environment keys;
- a clean-profile provider credential write with all three legacy master-key variables
  absent.

Stop rather than close if secure persistence cannot be implemented on a supported
release target without an undeclared external dependency.
