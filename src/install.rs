//! Installation-owned runfile resolution (self-contained-installation M002).
//!
//! The prebuilt release bundle carries `codegg`, `codegg-sandbox-helper`,
//! and `codegg-eggsearch` in one directory. This module resolves those
//! siblings relative to the canonical running `codegg` executable.
//!
//! Contract:
//! - `current_exe` canonical parent is authoritative. `cwd`, `PATH`, and
//!   inherited helper-specific environment variables are never consulted
//!   for the sibling location itself (PATH remains only a documented
//!   legacy/source-build compatibility fallback for eggsearch dispatch,
//!   never for the sandbox helper).
//! - Sibling candidates must be regular executable files inside the same
//!   installation directory (no symlink escape, no directory traversal).
//! - Missing/corrupt sidecars produce installation-specific diagnostics
//!   ("reinstall the prebuilt bundle"), never a bare "install eggsearch"
//!   remedy as the primary message.

use std::path::{Path, PathBuf};

/// Pinned upstream eggsearch version carried as `codegg-eggsearch`.
pub const PINNED_EGGSEARCH_VERSION: &str = "0.3.9";
/// Upstream eggsearch source for provenance diagnostics.
pub const PINNED_EGGSEARCH_SOURCE: &str = "https://github.com/eggstack/eggsearch";
/// In-process eggsact library baseline (no executable is ever resolved).
pub const PINNED_EGGSACT_VERSION: &str = "1.2.5";

/// Managed sidecar file stem (without platform suffix).
pub const EGGSEARCH_SIDECAR_STEM: &str = "codegg-eggsearch";
/// Trusted helper file stem (without platform suffix).
pub const SANDBOX_HELPER_STEM: &str = "codegg-sandbox-helper";

/// Platform-aware managed eggsearch file name.
pub fn eggsearch_sidecar_name() -> String {
    if cfg!(windows) {
        format!("{EGGSEARCH_SIDECAR_STEM}.exe")
    } else {
        EGGSEARCH_SIDECAR_STEM.to_string()
    }
}

/// Platform-aware sandbox helper file name.
pub fn sandbox_helper_name() -> String {
    if cfg!(windows) {
        format!("{SANDBOX_HELPER_STEM}.exe")
    } else {
        SANDBOX_HELPER_STEM.to_string()
    }
}

/// How the eggsearch command was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EggsearchResolution {
    /// Explicit `[search.eggsearch].command` override (advanced).
    ExplicitCommand { command: String },
    /// Installation-owned `codegg-eggsearch` sibling.
    ManagedSibling { path: PathBuf },
    /// Legacy `eggsearch` on `PATH` (source-build compatibility only).
    LegacyPath { command: String },
}

impl EggsearchResolution {
    /// Command string to spawn.
    pub fn command(&self) -> String {
        match self {
            Self::ExplicitCommand { command } | Self::LegacyPath { command } => command.clone(),
            Self::ManagedSibling { path } => path.to_string_lossy().into_owned(),
        }
    }

    /// Short source label for diagnostics (`doctor search`).
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::ExplicitCommand { .. } => "explicit-command",
            Self::ManagedSibling { .. } => "managed-sidecar",
            Self::LegacyPath { .. } => "legacy-path",
        }
    }
}

/// Resolve the installation directory for a given executable path.
///
/// Canonicalizes the executable, takes its parent, and canonicalizes the
/// parent. In test builds, Cargo places test executables under
/// `target/<profile>/deps`; the parent is then lifted one level so tests
/// resolve the same `target/<profile>` directory that holds the real
/// sibling helper built by `cargo build`.
pub fn installation_dir_for(current: &Path) -> Result<PathBuf, String> {
    let current = current
        .canonicalize()
        .map_err(|e| format!("CodeGG executable could not be resolved: {e}"))?;
    let install_root = current
        .parent()
        .ok_or_else(|| "CodeGG executable has no installation directory".to_string())?
        .canonicalize()
        .map_err(|e| format!("CodeGG installation directory could not be resolved: {e}"))?;
    #[cfg(test)]
    {
        if install_root.file_name().is_some_and(|name| name == "deps") {
            return install_root
                .parent()
                .ok_or_else(|| "Cargo test executable has no target directory".to_string())?
                .canonicalize()
                .map_err(|e| format!("Cargo target directory could not be resolved: {e}"));
        }
    }
    Ok(install_root)
}

/// Resolve the installation directory of the running executable.
pub fn installation_dir() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    installation_dir_for(&current)
}

/// Strict sibling validation shared by the sandbox helper and the managed
/// eggsearch sidecar: the candidate must canonicalize to a regular
/// executable file directly inside `install_root` (no escape, no symlink
/// indirection outside the directory).
pub fn validate_installation_sibling(install_root: &Path, name: &str) -> Result<PathBuf, String> {
    let candidate = install_root.join(name);
    let resolved = candidate
        .canonicalize()
        .map_err(|e| format!("installation-owned {name} could not be resolved: {e}"))?;
    if resolved.parent() != Some(install_root) {
        return Err(format!(
            "installation-owned {name} escaped the installation directory"
        ));
    }
    let metadata = std::fs::metadata(&resolved)
        .map_err(|e| format!("installation-owned {name} metadata unavailable: {e}"))?;
    if !metadata.file_type().is_file() {
        return Err(format!("installation-owned {name} is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("installation-owned {name} is not executable"));
        }
    }
    Ok(resolved)
}

/// Resolve the managed `codegg-eggsearch` sibling, if present and valid.
pub fn managed_eggsearch_path() -> Result<PathBuf, String> {
    let dir = installation_dir()?;
    validate_installation_sibling(&dir, &eggsearch_sidecar_name())
}

/// Resolve the managed sibling for an explicit executable location.
/// Test seam: production calls [`managed_eggsearch_path`].
pub fn managed_eggsearch_path_for(current_exe: &Path) -> Result<PathBuf, String> {
    let dir = installation_dir_for(current_exe)?;
    validate_installation_sibling(&dir, &eggsearch_sidecar_name())
}

/// Resolve the trusted sandbox helper sibling (strict; no PATH fallback).
pub fn trusted_sandbox_helper_path() -> Result<PathBuf, String> {
    let dir = installation_dir()?;
    validate_installation_sibling(&dir, &sandbox_helper_name())
}

/// Resolve the trusted helper for an explicit executable location.
/// Test seam shared with [`installation_dir_for`].
pub fn trusted_sandbox_helper_path_for(current_exe: &Path) -> Result<PathBuf, String> {
    let dir = installation_dir_for(current_exe)?;
    validate_installation_sibling(&dir, &sandbox_helper_name())
}

/// Resolve the eggsearch spawn command per the M002 contract:
///
/// 1. explicit `[search.eggsearch].command` wins when set;
/// 2. otherwise the installation-owned `codegg-eggsearch` sibling wins;
/// 3. otherwise fall back to legacy `eggsearch` on `PATH` (source-build
///    compatibility; a prebuilt release test must never depend on it).
///
/// `cwd` and helper-specific environment variables are never consulted.
pub fn resolve_eggsearch_command(
    egg_cfg: &codegg_config::schema::EggsearchConfig,
) -> EggsearchResolution {
    if let Some(explicit) = egg_cfg.explicit_command() {
        return EggsearchResolution::ExplicitCommand {
            command: explicit.to_string(),
        };
    }
    if let Ok(path) = managed_eggsearch_path() {
        return EggsearchResolution::ManagedSibling { path };
    }
    EggsearchResolution::LegacyPath {
        command: "eggsearch".to_string(),
    }
}

/// Test seam for [`resolve_eggsearch_command`] with an explicit executable.
pub fn resolve_eggsearch_command_for(
    egg_cfg: &codegg_config::schema::EggsearchConfig,
    current_exe: &Path,
) -> EggsearchResolution {
    if let Some(explicit) = egg_cfg.explicit_command() {
        return EggsearchResolution::ExplicitCommand {
            command: explicit.to_string(),
        };
    }
    if let Ok(path) = managed_eggsearch_path_for(current_exe) {
        return EggsearchResolution::ManagedSibling { path };
    }
    EggsearchResolution::LegacyPath {
        command: "eggsearch".to_string(),
    }
}

/// Presence state of one expected runfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiblingState {
    Present { path: PathBuf },
    Missing { expected: PathBuf },
    Invalid { expected: PathBuf, reason: String },
}

/// Describe one expected sibling without spawning it.
pub fn describe_sibling(name: &str) -> SiblingState {
    let expected = installation_dir()
        .map(|d| d.join(name))
        .unwrap_or_else(|_| PathBuf::from(name));
    match installation_dir().and_then(|d| validate_installation_sibling(&d, name)) {
        Ok(path) => SiblingState::Present { path },
        Err(reason) => {
            // Distinguish "file simply absent" from "present but invalid".
            if expected.exists() || expected.is_symlink() {
                SiblingState::Invalid { expected, reason }
            } else {
                // The validator's canonicalize error already covers absence;
                // surface the expected path plus the reason for doctor.
                SiblingState::Missing { expected }
            }
        }
    }
}

/// Probe `codegg-eggsearch --version` with a bounded blocking spawn.
/// Returns the raw first line on success (e.g. `eggsearch 0.3.9`).
pub fn probe_eggsearch_version(sidecar: &Path) -> Result<String, String> {
    let output = std::process::Command::new(sidecar)
        .arg("--version")
        .output()
        .map_err(|e| format!("managed eggsearch sidecar could not be executed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "managed eggsearch sidecar --version exited with {}",
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().unwrap_or("").trim().to_string();
    if first.is_empty() {
        return Err("managed eggsearch sidecar --version produced no output".to_string());
    }
    Ok(first)
}

/// Check that a `--version` line identifies the pinned eggsearch release.
pub fn check_eggsearch_version_output(output: &str) -> Result<String, String> {
    let line = output.lines().next().unwrap_or("").trim();
    if !line.contains("eggsearch") {
        return Err(format!(
            "managed eggsearch sidecar identity rejected (expected eggsearch, got: {line:?})"
        ));
    }
    if !line.contains(PINNED_EGGSEARCH_VERSION) {
        return Err(format!(
            "managed eggsearch sidecar version mismatch (expected {PINNED_EGGSEARCH_VERSION}, got: {line:?})"
        ));
    }
    Ok(line.to_string())
}

/// Installation-specific actionable hint when the managed sidecar cannot
/// be used. Never offers bare "install eggsearch" as the primary remedy.
pub fn managed_sidecar_hint(state: &SiblingState) -> String {
    match state {
        SiblingState::Missing { expected } => format!(
            "managed sidecar missing at {}: reinstall the prebuilt CodeGG bundle (installer installs codegg, codegg-sandbox-helper, codegg-eggsearch together); advanced override remains via [search.eggsearch].command or [mcp.eggsearch]",
            expected.display()
        ),
        SiblingState::Invalid { expected, reason } => format!(
            "managed sidecar invalid at {} ({reason}): reinstall the prebuilt CodeGG bundle; advanced override remains via [search.eggsearch].command or [mcp.eggsearch]",
            expected.display()
        ),
        SiblingState::Present { path } => format!(
            "managed sidecar present at {} (expected eggsearch {PINNED_EGGSEARCH_VERSION} from {PINNED_EGGSOURCE}); if startup still fails, reinstall the bundle before configuring overrides",
            path.display(),
            PINNED_EGGSOURCE = PINNED_EGGSEARCH_SOURCE,
        ),
    }
}

/// One-line eggsact contract indication: in-process library path only.
/// There is deliberately no executable lookup — a clean host has no
/// `eggsact` binary and the curated tool set must still be exposed.
pub fn eggsact_contract_line() -> String {
    format!(
        "eggsact in-process library {PINNED_EGGSACT_VERSION} (no executable required; curated CodeGG palette only)"
    )
}

/// Renderable installation/runfile report lines (secret-free).
#[derive(Debug, Clone)]
pub struct InstallationReport {
    pub codegg_path: String,
    pub codegg_version: String,
    pub installation_dir: String,
    pub eggsearch_sidecar_name: String,
    pub eggsearch_state: SiblingState,
    pub eggsearch_version: Option<String>,
    pub eggsearch_version_error: Option<String>,
    pub sandbox_helper_name: String,
    pub sandbox_state: SiblingState,
    pub sandbox_available: bool,
    pub sandbox_probe: String,
    pub eggsact_line: String,
}

impl InstallationReport {
    pub fn describe() -> Self {
        let codegg_path = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<unresolved: {e}>"));
        let installation_dir = installation_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<unresolved: {e}>"));
        let eggsearch_name = eggsearch_sidecar_name();
        let helper_name = sandbox_helper_name();
        let eggsearch_state = describe_sibling(&eggsearch_name);
        let sandbox_state = describe_sibling(&helper_name);
        let (eggsearch_version, eggsearch_version_error) = match &eggsearch_state {
            SiblingState::Present { path } => match probe_eggsearch_version(path) {
                Ok(raw) => match check_eggsearch_version_output(&raw) {
                    Ok(checked) => (Some(checked), None),
                    Err(e) => (Some(raw), Some(e)),
                },
                Err(e) => (None, Some(e)),
            },
            SiblingState::Missing { .. } | SiblingState::Invalid { .. } => (None, None),
        };
        let sandbox_available = crate::security::sandbox::SandboxConfig::is_available();
        let sandbox_probe = match crate::security::sandbox::probe_landlock() {
            Ok(()) => "landlock probe: available".to_string(),
            Err(e) => format!("landlock probe: unavailable ({e})"),
        };
        Self {
            codegg_path,
            codegg_version: env!("CARGO_PKG_VERSION").to_string(),
            installation_dir,
            eggsearch_sidecar_name: eggsearch_name,
            eggsearch_state,
            eggsearch_version,
            eggsearch_version_error,
            sandbox_helper_name: helper_name,
            sandbox_state,
            sandbox_available,
            sandbox_probe,
            eggsact_line: eggsact_contract_line(),
        }
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("CodeGG executable: {}", self.codegg_path));
        lines.push(format!("CodeGG version: {}", self.codegg_version));
        lines.push(format!("Installation directory: {}", self.installation_dir));
        match &self.eggsearch_state {
            SiblingState::Present { path } => {
                lines.push(format!(
                    "Managed eggsearch ({}): present at {}",
                    self.eggsearch_sidecar_name,
                    path.display()
                ));
            }
            SiblingState::Missing { expected } => {
                lines.push(format!(
                    "Managed eggsearch ({}): MISSING (expected {})",
                    self.eggsearch_sidecar_name,
                    expected.display()
                ));
            }
            SiblingState::Invalid { expected, reason } => {
                lines.push(format!(
                    "Managed eggsearch ({}): INVALID at {} ({})",
                    self.eggsearch_sidecar_name,
                    expected.display(),
                    reason
                ));
            }
        }
        if let Some(v) = &self.eggsearch_version {
            lines.push(format!("Managed eggsearch version: {v}"));
        }
        if let Some(e) = &self.eggsearch_version_error {
            lines.push(format!("Managed eggsearch version probe: {e}"));
        }
        match &self.sandbox_state {
            SiblingState::Present { path } => {
                lines.push(format!(
                    "Sandbox helper ({}): present at {}",
                    self.sandbox_helper_name,
                    path.display()
                ));
            }
            SiblingState::Missing { expected } => {
                lines.push(format!(
                    "Sandbox helper ({}): MISSING (expected {})",
                    self.sandbox_helper_name,
                    expected.display()
                ));
            }
            SiblingState::Invalid { expected, reason } => {
                lines.push(format!(
                    "Sandbox helper ({}): INVALID at {} ({})",
                    self.sandbox_helper_name,
                    expected.display(),
                    reason
                ));
            }
        }
        lines.push(format!(
            "Sandbox enforcement: {} ({})",
            if self.sandbox_available {
                "supported kernel"
            } else {
                "unsupported kernel/platform (fallback behavior applies)"
            },
            self.sandbox_probe
        ));
        lines.push(self.eggsact_line.clone());
        // Actionable hint for missing/corrupt managed sidecar.
        if !matches!(&self.eggsearch_state, SiblingState::Present { .. })
            || self.eggsearch_version_error.is_some()
        {
            lines.push(managed_sidecar_hint(&self.eggsearch_state));
        }
        if !matches!(&self.sandbox_state, SiblingState::Present { .. }) {
            lines.push(format!(
                "sandbox helper {}: reinstall the prebuilt bundle; no PATH fallback exists by design",
                self.sandbox_helper_name
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_installation(exe_name: &str, helper: bool, egg: bool) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("installation fixture");
        let exe = dir.path().join(exe_name);
        std::fs::write(&exe, b"codegg").expect("exe fixture");
        if helper {
            let h = dir.path().join(sandbox_helper_name());
            std::fs::write(&h, b"helper").expect("helper fixture");
            std::fs::set_permissions(&h, std::fs::Permissions::from_mode(0o755))
                .expect("helper perms");
        }
        if egg {
            let e = dir.path().join(eggsearch_sidecar_name());
            std::fs::write(&e, b"egg").expect("egg fixture");
            std::fs::set_permissions(&e, std::fs::Permissions::from_mode(0o755))
                .expect("egg perms");
        }
        dir
    }

    #[test]
    fn absent_command_prefers_managed_sibling() {
        let cfg = codegg_config::schema::EggsearchConfig::default();
        assert!(!cfg.has_explicit_command());
        assert!(cfg.explicit_command().is_none());
        // With no real sibling assertions here: the contract is that an
        // absent command never short-circuits to legacy when a sibling
        // exists (covered by the `for` variant below).
        let _ = resolve_eggsearch_command(&cfg).source_label();
    }

    #[cfg(unix)]
    #[test]
    fn absent_command_resolves_managed_sibling_while_explicit_wins() {
        let dir = make_installation("codegg", true, true);
        let exe = dir.path().join("codegg");

        let absent = codegg_config::schema::EggsearchConfig::default();
        let resolved = resolve_eggsearch_command_for(&absent, &exe);
        match resolved {
            EggsearchResolution::ManagedSibling { path } => {
                assert!(path.ends_with(eggsearch_sidecar_name()));
            }
            other => panic!("absent command must resolve managed sibling, got {other:?}"),
        }

        let explicit = codegg_config::schema::EggsearchConfig {
            command: Some("my-eggsearch".to_string()),
            ..Default::default()
        };
        assert!(explicit.has_explicit_command());
        let resolved = resolve_eggsearch_command_for(&explicit, &exe);
        assert_eq!(
            resolved,
            EggsearchResolution::ExplicitCommand {
                command: "my-eggsearch".to_string(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_sibling_falls_back_to_legacy_path() {
        let dir = make_installation("codegg", true, false);
        let exe = dir.path().join("codegg");
        let absent = codegg_config::schema::EggsearchConfig::default();
        let resolved = resolve_eggsearch_command_for(&absent, &exe);
        assert_eq!(
            resolved,
            EggsearchResolution::LegacyPath {
                command: "eggsearch".to_string(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolver_ignores_cwd_and_helper_env() {
        let dir = make_installation("codegg", true, true);
        let exe = dir.path().join("codegg");
        let decoy_dir = tempfile::tempdir().expect("decoy");
        let decoy = decoy_dir.path().join(eggsearch_sidecar_name());
        std::fs::write(&decoy, b"decoy").expect("decoy");
        std::env::set_var("CODEGG_SANDBOX_HELPER", &decoy);
        std::env::set_var("CODEGG_EGGSEARCH_PATH", &decoy);
        let absent = codegg_config::schema::EggsearchConfig::default();
        let resolved = resolve_eggsearch_command_for(&absent, &exe);
        std::env::remove_var("CODEGG_SANDBOX_HELPER");
        std::env::remove_var("CODEGG_EGGSEARCH_PATH");
        match resolved {
            EggsearchResolution::ManagedSibling { path } => {
                assert!(path.starts_with(dir.path().canonicalize().unwrap()));
                assert!(!path.starts_with(decoy_dir.path().canonicalize().unwrap()));
            }
            other => panic!("env must not steer resolution, got {other:?}"),
        }
    }

    #[test]
    fn missing_sidecar_hint_names_reinstall_not_bare_install() {
        let state = SiblingState::Missing {
            expected: PathBuf::from("/opt/codegg/codegg-eggsearch"),
        };
        let hint = managed_sidecar_hint(&state);
        assert!(hint.contains("reinstall the prebuilt"));
        assert!(!hint
            .trim_start()
            .to_lowercase()
            .starts_with("install eggsearch"));
    }

    #[test]
    fn version_check_accepts_pin_and_rejects_drift() {
        assert!(check_eggsearch_version_output("eggsearch 0.3.9").is_ok());
        assert!(check_eggsearch_version_output("eggsearch 0.3.8").is_err());
        assert!(check_eggsearch_version_output("something-else 0.3.9").is_err());
    }
}
