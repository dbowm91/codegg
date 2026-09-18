//! Master-key resolution for CodeGG's encrypted secret stores.
//!
//! Resolution precedence (read path, side-effect free):
//!
//! 1. `CODEGG_MASTER_KEY`,
//! 2. `CODEGG_ENCRYPTION_KEY`,
//! 3. `OPENCODE_ENCRYPTION_KEY`,
//! 4. the CodeGG-managed user-local key at
//!    `<config_dir>/codegg/master.key` (see [`managed_key_path`]).
//!
//! [`get_master_key`] implements exactly this chain and never creates a
//! key. Secret-store write paths that create new protected material must
//! use [`get_or_create_master_key`] (default locations) or
//! [`get_or_create_master_key_at`] (explicit managed-key path with a
//! caller-provided freshness bit) so a first-run install self-initializes
//! while pre-existing encrypted material never triggers a silent key
//! replacement.
//!
//! Managed-key file contract:
//!
//! - Location: canonical CodeGG user config root
//!   (`dirs::config_dir()/codegg/master.key`; `~/.config/codegg/master.key`
//!   on Linux, `~/Library/Application Support/codegg/master.key` on macOS,
//!   `%APPDATA%/codegg/master.key` on Windows). `CODEGG_MASTER_KEY_FILE`
//!   overrides the path (isolation/testing support). The key is never
//!   stored in a project/workspace directory or the current working
//!   directory.
//! - Content: 32 CSPRNG bytes (256 bits), hex-encoded to 64 characters.
//! - Unix permissions: parent created `0o700` when newly created; key file
//!   created atomically with `O_EXCL` (`create_new`) at `0o600` and then
//!   `fsync`ed (plus a parent-dir `fsync`). Reads reject symlinks,
//!   non-regular files, and any group/other permission bits, failing
//!   closed instead of repairing an attacker-controlled path.
//! - Windows: the file is created with a single `create_new` write inside
//!   the user profile (`%APPDATA%/codegg`), so it inherits the profile's
//!   user-private ACL and is never made world-readable. No external
//!   command is used on any platform.
//! - The key value is never printed, logged, serialized into normal
//!   config, exported with sessions, or included in diagnostics. Debug
//!   impls for key-carrying types are redacted.
//!
//! Do not create a key at process startup. Reads stay side-effect free;
//! only secret-store writes bootstrap.
//!
//! The encryption helpers at the bottom of this file remain intentionally
//! no-ops. The real encryption pipeline lives in `codegg-providers`
//! (root crate's `crypto` module) and is driven by the credential store
//! and typed `AuthConfig::ApiKey.encrypted_value` fields during
//! credential resolution.

use crate::error::AppError;
use crate::schema::Config;
use std::path::{Path, PathBuf};

/// Env chain for an explicitly deployment-managed master key, in
/// precedence order.
const ENV_CHAIN: [(&str, MasterKeySource); 3] = [
    (
        "CODEGG_MASTER_KEY",
        MasterKeySource::Environment("CODEGG_MASTER_KEY"),
    ),
    (
        "CODEGG_ENCRYPTION_KEY",
        MasterKeySource::Environment("CODEGG_ENCRYPTION_KEY"),
    ),
    (
        "OPENCODE_ENCRYPTION_KEY",
        MasterKeySource::Environment("OPENCODE_ENCRYPTION_KEY"),
    ),
];

/// Override for the managed-key file location (isolation/testing).
pub const MANAGED_KEY_FILE_ENV: &str = "CODEGG_MASTER_KEY_FILE";
/// Stable managed-key filename inside the CodeGG user config root.
pub const MANAGED_KEY_FILENAME: &str = "master.key";
/// Stable credential-store filename inside the same root.
pub const CREDENTIALS_FILENAME: &str = "credentials.json";
/// Stable MCP OAuth token-store filename inside the same root.
pub const MCP_TOKENS_FILENAME: &str = "mcp_tokens.json";
/// Legacy MCP v1 envelope prefix (decrypt-only compatibility reader).
const LEGACY_MCP_MAGIC: &str = "CODEGG_ENC_v1";
/// Current MCP v2 envelope prefix (canonical crypto).
const MCP_V2_MAGIC: &str = "CODEGG_MCP_ENC_v2:";
/// Managed key = 32 random bytes, hex-encoded.
const MANAGED_KEY_BYTES: usize = 32;
const MANAGED_KEY_HEX_LEN: usize = MANAGED_KEY_BYTES * 2;

/// Where a resolved master key came from. Safe to log: labels only, never
/// the key value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterKeySource {
    Environment(&'static str),
    Managed,
}

impl MasterKeySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            MasterKeySource::Environment(name) => name,
            MasterKeySource::Managed => "managed(master.key)",
        }
    }
}

/// A resolved master key. The value is only available via [`Self::expose`];
/// `Debug`/`Display` are redacted so the key can never appear in
/// debug/error/output snapshots.
#[derive(Clone)]
pub struct ManagedMasterKey {
    key: String,
    source: MasterKeySource,
}

impl ManagedMasterKey {
    pub fn expose(&self) -> &str {
        &self.key
    }

    pub fn source(&self) -> MasterKeySource {
        self.source
    }
}

impl std::fmt::Debug for ManagedMasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedMasterKey")
            .field("source", &self.source.as_str())
            .field("key", &"[redacted]")
            .finish()
    }
}

/// Failures from the create-on-write resolver. Display text is actionable
/// recovery guidance and never contains key material.
#[derive(Debug)]
pub enum MasterKeyError {
    /// No usable key exists and the target store already holds encrypted
    /// material. Creating a fresh key would orphan that ciphertext, so the
    /// caller must restore the historical environment key instead.
    MissingWithExistingCiphertext {
        managed_path: PathBuf,
    },
    /// The managed-key file exists but is unsafe (symlink, non-regular, or
    /// group/other-readable). Refusing to use or silently repair it.
    UnsafeKeyFile {
        path: PathBuf,
        reason: &'static str,
    },
    /// The managed-key file exists but does not parse as a CodeGG-managed
    /// key. Refusing to overwrite it.
    CorruptKeyFile {
        path: PathBuf,
    },
    /// No user config directory could be determined and no
    /// `CODEGG_MASTER_KEY_FILE` override was set.
    NoConfigDir,
    Io(std::io::Error),
}

impl std::fmt::Display for MasterKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MasterKeyError::MissingWithExistingCiphertext { managed_path } => write!(
                f,
                "no usable master key and existing encrypted credential material was found; \
                 refusing to create a new managed key at '{}' because it would orphan existing \
                 ciphertext; restore the historical key via CODEGG_MASTER_KEY \
                 (CODEGG_ENCRYPTION_KEY / OPENCODE_ENCRYPTION_KEY aliases) that encrypted the \
                 existing store, or, only if the old data is expendable, remove the existing \
                 encrypted store files and retry",
                managed_path.display()
            ),
            MasterKeyError::UnsafeKeyFile { path, reason } => write!(
                f,
                "managed master-key file at '{}' is unsafe ({reason}); refusing to use or \
                 overwrite it; verify ownership and permissions before retrying",
                path.display()
            ),
            MasterKeyError::CorruptKeyFile { path } => write!(
                f,
                "managed master-key file at '{}' is corrupt; refusing to overwrite it because \
                 existing encrypted stores may depend on it; restore the file from backup or, \
                 only if no encrypted store depends on it, remove it and retry",
                path.display()
            ),
            MasterKeyError::NoConfigDir => write!(
                f,
                "could not determine user config directory for the managed master key; set \
                 CODEGG_MASTER_KEY or CODEGG_MASTER_KEY_FILE"
            ),
            MasterKeyError::Io(e) => write!(f, "master-key storage error: {e}"),
        }
    }
}

impl std::error::Error for MasterKeyError {}

impl From<std::io::Error> for MasterKeyError {
    fn from(e: std::io::Error) -> Self {
        MasterKeyError::Io(e)
    }
}

/// Read-only resolver: environment compatibility chain first, then an
/// existing CodeGG-managed user-local key. Side-effect free: never creates
/// a key, never writes to disk.
pub fn get_master_key() -> Option<String> {
    master_key_with_source().map(|(key, _)| key)
}

/// Read-only resolver with source label. Side-effect free.
pub fn master_key_with_source() -> Option<(String, MasterKeySource)> {
    for (name, source) in ENV_CHAIN {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some((value, source));
            }
        }
    }
    let path = managed_key_path()?;
    match read_managed_key_file(&path) {
        Ok(key) => Some((key, MasterKeySource::Managed)),
        Err(_) => None,
    }
}

/// Canonical managed-key path: `CODEGG_MASTER_KEY_FILE` override when set,
/// otherwise `<config_dir>/codegg/master.key`. Returns `None` when no user
/// config directory is available and no override is set.
pub fn managed_key_path() -> Option<PathBuf> {
    if let Ok(value) = std::env::var(MANAGED_KEY_FILE_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    default_managed_key_path()
}

fn default_managed_key_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("codegg").join(MANAGED_KEY_FILENAME))
}

/// Default credential-store path shared with the providers crate
/// (`<config_dir>/codegg/credentials.json`).
pub fn default_credential_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("codegg").join(CREDENTIALS_FILENAME))
}

/// Default MCP OAuth token-store path shared with `src/mcp/auth.rs`
/// (`<config_dir>/codegg/mcp_tokens.json`).
pub fn default_mcp_token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("codegg").join(MCP_TOKENS_FILENAME))
}

/// Create-on-write resolver for the default locations. Returns the
/// explicit environment key when configured, reuses an existing managed
/// key, or atomically creates a cryptographically random managed key when
/// no protected material exists yet. When default credential or MCP token
/// material already exists and no key is available, returns
/// [`MasterKeyError::MissingWithExistingCiphertext`] instead of
/// generating a replacement.
pub fn get_or_create_master_key() -> Result<ManagedMasterKey, MasterKeyError> {
    for (name, source) in ENV_CHAIN {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Ok(ManagedMasterKey { key: value, source });
            }
        }
    }
    let path = managed_key_path().ok_or(MasterKeyError::NoConfigDir)?;
    match read_managed_key_file(&path) {
        Ok(key) => Ok(ManagedMasterKey {
            key,
            source: MasterKeySource::Managed,
        }),
        Err(ReadKeyOutcome::Missing) => {
            if has_default_protected_material() {
                return Err(MasterKeyError::MissingWithExistingCiphertext { managed_path: path });
            }
            let key = create_managed_key_file(&path)?;
            Ok(ManagedMasterKey {
                key,
                source: MasterKeySource::Managed,
            })
        }
        Err(ReadKeyOutcome::Failure(e)) => Err(e),
    }
}

/// Create-on-write resolver for callers with an explicit store-freshness
/// bit (custom credential/token paths, isolated test roots).
///
/// `is_store_fresh` must be true only when the caller's own store holds no
/// encrypted material that the new key would orphan. When the managed path
/// is the canonical default, the other default store (credential vs. MCP
/// token) is also checked so one fresh store cannot orphan the other's
/// ciphertext.
pub fn get_or_create_master_key_for_store(
    is_store_fresh: bool,
) -> Result<ManagedMasterKey, MasterKeyError> {
    let path = managed_key_path().ok_or(MasterKeyError::NoConfigDir)?;
    get_or_create_master_key_at(&path, is_store_fresh)
}

/// Create-on-write resolver at an explicit managed-key path (tests and
/// custom roots). See [`get_or_create_master_key_for_store`].
pub fn get_or_create_master_key_at(
    path: &Path,
    is_store_fresh: bool,
) -> Result<ManagedMasterKey, MasterKeyError> {
    for (name, source) in ENV_CHAIN {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Ok(ManagedMasterKey { key: value, source });
            }
        }
    }
    match read_managed_key_file(path) {
        Ok(key) => Ok(ManagedMasterKey {
            key,
            source: MasterKeySource::Managed,
        }),
        Err(ReadKeyOutcome::Missing) => {
            let fresh = if is_default_managed_path(path) {
                is_store_fresh && !has_default_protected_material()
            } else {
                is_store_fresh
            };
            if !fresh {
                return Err(MasterKeyError::MissingWithExistingCiphertext {
                    managed_path: path.to_path_buf(),
                });
            }
            let key = create_managed_key_file(path)?;
            Ok(ManagedMasterKey {
                key,
                source: MasterKeySource::Managed,
            })
        }
        Err(ReadKeyOutcome::Failure(e)) => Err(e),
    }
}

fn is_default_managed_path(path: &Path) -> bool {
    match default_managed_key_path() {
        Some(default) => default == path,
        None => false,
    }
}

enum ReadKeyOutcome {
    Missing,
    Failure(MasterKeyError),
}

/// Read and validate an existing managed-key file. Never creates anything.
fn read_managed_key_file(path: &Path) -> Result<String, ReadKeyOutcome> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ReadKeyOutcome::Missing);
        }
        Err(e) => {
            return Err(ReadKeyOutcome::Failure(MasterKeyError::Io(e)));
        }
    };
    if meta.file_type().is_symlink() {
        return Err(ReadKeyOutcome::Failure(MasterKeyError::UnsafeKeyFile {
            path: path.to_path_buf(),
            reason: "symlink",
        }));
    }
    if !meta.file_type().is_file() {
        return Err(ReadKeyOutcome::Failure(MasterKeyError::UnsafeKeyFile {
            path: path.to_path_buf(),
            reason: "not a regular file",
        }));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(ReadKeyOutcome::Failure(MasterKeyError::UnsafeKeyFile {
                path: path.to_path_buf(),
                reason: "group/other-readable permissions",
            }));
        }
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| ReadKeyOutcome::Failure(MasterKeyError::Io(e)))?;
    let key = content.trim().to_string();
    if !is_valid_managed_key(&key) {
        return Err(ReadKeyOutcome::Failure(MasterKeyError::CorruptKeyFile {
            path: path.to_path_buf(),
        }));
    }
    Ok(key)
}

fn is_valid_managed_key(key: &str) -> bool {
    key.len() == MANAGED_KEY_HEX_LEN && hex::decode(key).is_ok_and(|b| b.len() == MANAGED_KEY_BYTES)
}

/// Atomically create a new managed key with at least 256 bits of CSPRNG
/// entropy. Uses `create_new` (`O_EXCL`) so two concurrent first writes
/// converge: the loser reads back the winner's key.
fn create_managed_key_file(path: &Path) -> Result<String, MasterKeyError> {
    let key_bytes: [u8; MANAGED_KEY_BYTES] = rand::random();
    let key = hex::encode(key_bytes);

    if let Some(parent) = path.parent() {
        let parent_existed = parent.exists();
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        if !parent_existed {
            use std::os::unix::fs::PermissionsExt;
            // Best-effort hardening of a newly created root; an existing
            // directory keeps its current mode.
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(key.as_bytes())?;
            file.sync_all()?;
            drop(file);
            #[cfg(unix)]
            if let Some(parent) = path.parent() {
                if let Ok(dir) = std::fs::File::open(parent) {
                    let _ = dir.sync_all();
                }
            }
            // Re-read through the validating path so the created file is
            // known-good before it is returned.
            read_managed_key_file(path).map_err(|outcome| match outcome {
                ReadKeyOutcome::Missing => MasterKeyError::CorruptKeyFile {
                    path: path.to_path_buf(),
                },
                ReadKeyOutcome::Failure(e) => e,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Lost the creation race: reuse the winner's key.
            read_managed_key_file(path).map_err(|outcome| match outcome {
                ReadKeyOutcome::Missing => MasterKeyError::Io(e),
                ReadKeyOutcome::Failure(err) => err,
            })
        }
        Err(e) => Err(MasterKeyError::Io(e)),
    }
}

/// True when either default protected store already holds encrypted
/// material that a fresh managed key would orphan.
pub fn has_default_protected_material() -> bool {
    let credential_has =
        default_credential_path().is_some_and(|p| credential_file_has_encrypted_material(&p));
    let mcp_has =
        default_mcp_token_path().is_some_and(|p| mcp_token_file_has_encrypted_material(&p));
    credential_has || mcp_has
}

/// True when a credential-store file contains at least one record with a
/// non-empty `encrypted_secret`.
pub fn credential_file_has_encrypted_material(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    if content.trim().is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        // Unparsable but non-empty store file: fail closed and treat as
        // material so creation cannot silently orphan unknown content.
        return true;
    };
    match value.get("records").and_then(|r| r.as_array()) {
        Some(records) => records.iter().any(|record| {
            record
                .get("encrypted_secret")
                .and_then(|s| s.as_str())
                .is_some_and(|s| !s.is_empty())
        }),
        None => content.contains("encrypted_secret"),
    }
}

/// True when an MCP token-store file contains v2 or legacy encrypted
/// material. Plaintext/empty files are not material.
pub fn mcp_token_file_has_encrypted_material(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    content.starts_with(MCP_V2_MAGIC) || content.contains(LEGACY_MCP_MAGIC)
}

pub fn decrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // Intentionally a no-op: see the file header.
    Ok(())
}

pub fn encrypt_provider_keys(_config: &mut Config) -> Result<(), AppError> {
    // Intentionally a no-op: see the file header.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        prev: Vec<(&'static str, Option<String>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn clean() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let names = [
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
                MANAGED_KEY_FILE_ENV,
            ];
            let prev = names
                .into_iter()
                .map(|name| {
                    let value = std::env::var(name).ok();
                    std::env::remove_var(name);
                    (name, value)
                })
                .collect();
            Self { prev, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in self.prev.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn temp_key_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("master.key")
    }

    #[test]
    fn get_master_key_is_side_effect_free_and_env_first() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);
        // No file is created by reads.
        assert!(get_master_key().is_none());
        assert!(!key_path.exists());
        // Env chain wins and never creates the managed file.
        std::env::set_var("CODEGG_MASTER_KEY", "env-master-sentinel");
        assert_eq!(get_master_key().as_deref(), Some("env-master-sentinel"));
        assert!(!key_path.exists());
    }

    #[test]
    fn create_then_resolve_is_deterministic() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);

        let created = get_or_create_master_key_for_store(true).expect("create");
        assert_eq!(created.source(), MasterKeySource::Managed);
        assert!(key_path.exists());
        // 256-bit entropy: 64 hex chars decoding to 32 bytes.
        assert_eq!(created.expose().len(), MANAGED_KEY_HEX_LEN);
        assert!(hex::decode(created.expose()).is_ok_and(|b| b.len() == MANAGED_KEY_BYTES));

        // Restart resolves the same key through the read-only path.
        let reread = get_master_key().expect("reread");
        assert_eq!(reread, created.expose().to_string());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
            assert!(std::fs::symlink_metadata(&key_path)
                .expect("symlink metadata")
                .file_type()
                .is_file());
        }
    }

    #[test]
    fn concurrent_first_writes_converge_on_one_key() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = key_path.clone();
                std::thread::spawn(move || get_or_create_master_key_at(&path, true).expect("race"))
            })
            .collect();
        let mut keys = handles
            .into_iter()
            .map(|h| h.join().expect("join").expose().to_string())
            .collect::<Vec<_>>();
        keys.dedup();
        assert_eq!(keys.len(), 1, "concurrent creates must converge");
    }

    #[test]
    fn missing_store_with_material_refuses_new_key() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);

        let err = get_or_create_master_key_at(&key_path, false).unwrap_err();
        assert!(
            matches!(err, MasterKeyError::MissingWithExistingCiphertext { .. }),
            "expected orphan guard, got {err:?}"
        );
        assert!(!key_path.exists(), "no key may be generated over material");
        assert!(!format!("{err}").contains("CODEGG_MASTER_KEY_FILE"));
    }

    #[test]
    fn symlink_and_unsafe_permissions_fail_closed() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);
        let created = get_or_create_master_key_for_store(true).expect("create");

        // Symlink replacement is rejected.
        let link_dir = tempfile::tempdir().expect("tmpdir");
        let link_path = link_dir.path().join("master.key");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&key_path, &link_path).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&key_path, &link_path).expect("symlink");
        let err = get_or_create_master_key_at(&link_path, true).unwrap_err();
        assert!(
            matches!(err, MasterKeyError::UnsafeKeyFile { .. }),
            "symlink must fail closed, got {err:?}"
        );
        assert!(get_master_key().is_some(), "real path still resolves");

        // Group/other-readable files fail closed instead of being repaired.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644))
                .expect("chmod");
            assert!(
                get_master_key().is_none(),
                "unsafe key must not resolve via reads"
            );
            let err = get_or_create_master_key_at(&key_path, true).unwrap_err();
            assert!(matches!(err, MasterKeyError::UnsafeKeyFile { .. }));
            // The guard repairs nothing silently: permissions are untouched.
            let mode = std::fs::metadata(&key_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o644);
            // Restore so the Drop cleanup can remove the tempdir on all
            // platforms without permission surprises.
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
                .expect("restore");
        }
        let _ = created;
    }

    #[test]
    fn key_value_never_appears_in_debug_or_error_output() {
        let _guard = EnvGuard::clean();
        let dir = tempfile::tempdir().expect("tmpdir");
        let key_path = temp_key_path(&dir);
        std::env::set_var(MANAGED_KEY_FILE_ENV, &key_path);
        let created = get_or_create_master_key_for_store(true).expect("create");

        assert!(!format!("{created:?}").contains(created.expose()));
        assert!(format!("{created:?}").contains("[redacted]"));
        let err = get_or_create_master_key_at(&dir.path().join("other.key"), false).unwrap_err();
        assert!(!format!("{err:?}").contains(created.expose()));
        assert!(!format!("{err}").contains(created.expose()));
    }

    #[test]
    fn material_detectors_distinguish_fresh_from_populated() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let fresh = dir.path().join("credentials.json");
        assert!(!credential_file_has_encrypted_material(&fresh));
        std::fs::write(&fresh, "{\"version\":1,\"records\":[]}").expect("write");
        assert!(!credential_file_has_encrypted_material(&fresh));
        std::fs::write(
            &fresh,
            "{\"version\":1,\"records\":[{\"encrypted_secret\":\"v2:abc\"}]}",
        )
        .expect("write");
        assert!(credential_file_has_encrypted_material(&fresh));

        // Unparsable non-empty content fails closed toward "has material".
        std::fs::write(&fresh, "not-json{{{").expect("write");
        assert!(credential_file_has_encrypted_material(&fresh));

        let tokens = dir.path().join("mcp_tokens.json");
        assert!(!mcp_token_file_has_encrypted_material(&tokens));
        std::fs::write(&tokens, "[]").expect("write");
        assert!(!mcp_token_file_has_encrypted_material(&tokens));
        std::fs::write(&tokens, "CODEGG_MCP_ENC_v2:v2:abc").expect("write");
        assert!(mcp_token_file_has_encrypted_material(&tokens));
    }
}
