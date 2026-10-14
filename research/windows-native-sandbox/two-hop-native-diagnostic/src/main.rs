//! C044 two-hop diagnostic runner (test only).
//!
//! Windows launch path is `cfg(windows)`-gated and runs only on a
//! disposable consented host. Every other host prints an unqualified
//! report without claiming any security result. Portable manifest/nonce
//! helpers are unit-tested on all hosts.

use serde_json::json;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "codegg.two_hop_c044.v1";

/// Immutable stage manifest: one-shot nonce, runner-owned directory, and
/// the exact probe inputs. The manifest is verified before either hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageManifest {
    pub nonce: String,
    pub stage_dir: PathBuf,
    pub probe_name: String,
    pub broad_read_name: String,
}

impl StageManifest {
    pub fn new(nonce: impl Into<String>, stage_dir: PathBuf) -> Result<Self, String> {
        let nonce = nonce.into();
        if nonce.len() < 16 || nonce.len() > 128 {
            return Err("nonce must be 16..=128 characters".to_string());
        }
        if !nonce.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err("nonce must be alphanumeric plus -/_".to_string());
        }
        if !stage_dir.is_absolute() {
            return Err("stage directory must be absolute".to_string());
        }
        Ok(Self {
            nonce,
            stage_dir,
            probe_name: "native_probe.exe".to_string(),
            broad_read_name: "broad-readable-fixture.bin".to_string(),
        })
    }

    pub fn entry_path(&self) -> PathBuf {
        self.stage_dir.join(format!("entry-{}.json", self.nonce))
    }

    pub fn proof_path(&self) -> PathBuf {
        self.stage_dir.join(format!("proof-{}.json", self.nonce))
    }

    /// Verify the manifest against the stage directory without trusting
    /// any child output: absolute runner-owned dir, no traversal names,
    /// nonce-bound entry/proof paths.
    pub fn verify(&self) -> Result<(), String> {
        if !self.stage_dir.is_absolute() {
            return Err("stage directory must be absolute".to_string());
        }
        for name in [&self.probe_name, &self.broad_read_name] {
            if name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(format!("manifest name escapes the stage: {name}"));
            }
        }
        for path in [self.entry_path(), self.proof_path()] {
            if !path.starts_with(&self.stage_dir) {
                return Err("nonce-bound path escapes the stage directory".to_string());
            }
        }
        Ok(())
    }
}

/// Classify a missing entry marker: loader/privilege/environment failure,
/// never a read-denial result.
pub fn classify_missing_entry(startup_error: Option<&str>) -> &'static str {
    let _ = startup_error;
    "blocked-before-entry: diagnose loader/privilege/environment; not a read denial"
}

fn unqualified_report(reason: &str) {
    println!(
        "C044_REPORT={}",
        json!({"schema": SCHEMA, "qualified": false, "reason": reason})
    );
}

#[cfg(windows)]
fn run() {
    // Full two-hop launch (CreateProcessWithLogonW first hop, restricted
    // own-primary-token second hop via CreateRestrictedToken +
    // CreateProcessAsUserW, Job/suspend/resume verification, ordered
    // read/write probes, OS process-query cleanup) executes only on the
    // disposable consented Windows host per RUN.md. This stub records the
    // harness identity so a non-host run cannot be mistaken for evidence.
    unqualified_report("disposable consented Windows host required; see RUN.md");
}

fn main() {
    if !cfg!(windows) {
        unqualified_report("Windows-only diagnostic; no claim on this host");
        return;
    }
    #[cfg(windows)]
    run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_short_or_nonportable_nonce() {
        let dir = Path::new("/tmp").join("c044-stage");
        assert!(StageManifest::new("short", dir.clone()).is_err());
        assert!(StageManifest::new("nonce with spaces!!!!", dir.clone()).is_err());
        assert!(StageManifest::new("0123456789abcdef", PathBuf::from("relative")).is_err());
    }

    #[test]
    fn manifest_paths_stay_nonce_bound_inside_the_stage() {
        let dir = PathBuf::from(if cfg!(windows) {
            "C:\\c044-stage"
        } else {
            "/tmp/c044-stage"
        });
        let manifest =
            StageManifest::new("0123456789abcdef", dir.clone()).expect("valid manifest");
        manifest.verify().expect("valid manifest verifies");
        assert!(manifest.entry_path().starts_with(&dir));
        assert!(manifest.proof_path().starts_with(&dir));
        assert_ne!(manifest.entry_path(), manifest.proof_path());
    }

    #[test]
    fn manifest_rejects_traversal_probe_names() {
        // `/tmp` is not absolute on Windows (no drive prefix); use a
        // platform-absolute stage dir like the neighboring test.
        let dir = PathBuf::from(if cfg!(windows) {
            "C:\\c044-stage"
        } else {
            "/tmp/c044-stage"
        });
        let mut manifest =
            StageManifest::new("0123456789abcdef", dir).expect("valid manifest");
        manifest.probe_name = "../escape.exe".to_string();
        assert!(manifest.verify().is_err());
    }

    #[test]
    fn missing_entry_is_never_a_read_denial() {
        assert!(
            classify_missing_entry(None).contains("not a read denial"),
            "a child that never reaches entry proves nothing about reads"
        );
    }
}
