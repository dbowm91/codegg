//! Host-visible, non-installing diagnostics for the optional Playwright bundles.
//!
//! Detection intentionally inspects PATH only. Version probing would execute
//! Node/npm outside the scheduler-owned command path, so callers should use
//! the ordinary shell/doctor execution surface when they explicitly request a
//! version check.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaywrightSupportReport {
    pub node_path: Option<PathBuf>,
    pub cli_path: Option<PathBuf>,
    pub npx_path: Option<PathBuf>,
    pub configured_mode: &'static str,
    pub version_probe: &'static str,
}

impl PlaywrightSupportReport {
    pub fn detect(configured_mode: &'static str) -> Self {
        Self {
            node_path: find_on_path("node"),
            cli_path: find_on_path("playwright-cli").or_else(|| find_on_path("playwright")),
            npx_path: find_on_path("npx"),
            configured_mode,
            version_probe: "not executed; use explicit doctor/shell action",
        }
    }

    pub fn cli_available(&self) -> bool {
        self.cli_path.is_some() || (self.node_path.is_some() && self.npx_path.is_some())
    }
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for extension in [".exe", ".cmd", ".bat"] {
            let candidate = directory.join(format!("{command}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_never_claims_a_version_without_running_a_command() {
        let report = PlaywrightSupportReport::detect("none");
        assert_eq!(
            report.version_probe,
            "not executed; use explicit doctor/shell action"
        );
    }
}
