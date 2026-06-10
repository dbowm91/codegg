//! Security module — boundary between Codegg and the `eggsentry` crate.
//!
//! ## Codegg owns (this crate)
//!
//! - `policy` — permission policy / escalation
//! - `sandbox` — Landlock filesystem sandboxing
//! - `service` — high-level security service (Codegg-side glue)
//! - `ssrf` — SSRF protection (URL allow-listing, DNS re-validation)
//! - sensitive path matching
//!
//! ## `eggsentry` crate owns
//!
//! - command classification (`classify_bash_command`,
//!   `classify_git_subcommand`, `classify_tool_call`)
//! - secret / text scanning (`inspect_text`, `inspect_file`)
//! - dependency file detection and recommended audit commands
//! - `ProfileRunner` (deterministic profile scans)
//! - finding types (`SecurityFinding`, `SecurityReport`,
//!   `Severity`, `Confidence`, `SecurityCategory`)
//!
//! The re-exports below preserve the old `crate::security::finding::...`
//! path for any call site that has not yet been migrated. New code
//! should prefer direct `eggsentry::...` imports. See
//! `plans/native_tool_crates_hardening.md` Phase 8.

pub mod policy;
pub mod sandbox;
pub mod service;
pub mod ssrf;

pub mod command {
    pub use eggsentry::command::*;
}
pub mod dependency {
    pub use eggsentry::dependency::*;
}
pub mod finding {
    pub use eggsentry::finding::*;
}
pub mod profile {
    pub use eggsentry::profile::*;
}
pub mod scanner {
    pub use eggsentry::scanner::*;
}

pub use sandbox::{
    get_default_allowed_paths, get_sensitive_paths, validate_path_safety, SandboxConfig,
};
pub use ssrf::{
    ipv6_segments_to_ipv4, is_internal_ip, revalidate_dns, validate_host_ip, validate_url_host,
};

use globset::Glob;
use std::path::Path;

use crate::config::schema::SensitivePathConfig;

/// Check if a file path matches any configured sensitive path patterns.
/// Returns the matching config (with reason/review_level) if a match is found.
pub fn matches_sensitive_path<'a>(
    file_path: Option<&str>,
    sensitive_paths: &'a [SensitivePathConfig],
) -> Option<&'a SensitivePathConfig> {
    let raw_path = file_path?;
    if sensitive_paths.is_empty() {
        return None;
    }

    let path = Path::new(raw_path);
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_str = canonical.to_string_lossy();

    for config in sensitive_paths {
        if config.glob.is_empty() {
            continue;
        }
        if let Ok(glob) = Glob::new(&config.glob) {
            let matcher = glob.compile_matcher();
            if matcher.is_match(&*canonical_str) {
                return Some(config);
            }
        }
    }
    None
}
