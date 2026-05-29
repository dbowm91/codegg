pub mod command;
pub mod dependency;
pub mod finding;
pub mod policy;
pub mod profile;
pub mod sandbox;
pub mod scanner;
pub mod service;
pub mod ssrf;

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
    let Some(raw_path) = file_path else {
        return None;
    };
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
