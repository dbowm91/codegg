//! Local-only Git repository lineage evidence.
//!
//! This module deliberately does not resolve remotes, contact hooks, or inspect
//! filesystem paths beyond asking Git about the repository rooted at the path
//! supplied by the caller.  The equality key is made only from normalized
//! remote components, so it is stable when the same checkout moves on disk.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};

/// Maximum amount of `git config` output retained by the local probe.
pub const MAX_CONFIG_OUTPUT_BYTES: usize = 64 * 1024;

/// A remote after transport-specific decoration has been removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedRemote {
    /// Lowercase host (including an explicit port when one was supplied).
    pub host: String,
    /// Repository path without a leading or trailing slash and without `.git`.
    pub path: String,
}

impl NormalizedRemote {
    /// Returns the path-independent key used to compare repository lineage.
    pub fn equality_key(&self) -> String {
        format!("git:{}/{}", self.host, self.path)
    }
}

/// Result of normalizing one remote URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "classification", content = "value", rename_all = "snake_case")]
pub enum RemoteNormalization {
    /// The remote is safe and usable as lineage evidence.
    Usable(NormalizedRemote),
    /// Credential-bearing material was found; no source value is retained.
    Redacted,
    /// The value is not a supported, well-formed remote.
    Invalid,
}

/// Why otherwise-local evidence cannot be used as a unique lineage identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsufficientReason {
    /// At least one remote contained credentials or a recognized secret field.
    RedactedRemote,
    /// At least one remote was malformed or unsupported.
    InvalidRemote,
    /// Git config output exceeded the local probe bound.
    OutputBoundExceeded,
    /// A config record could not be interpreted as a remote URL.
    MalformedConfig,
}

/// Bounded evidence about the lineage of one workspace path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "classification", rename_all = "snake_case")]
pub enum RepositoryLineageEvidence {
    /// The registered workspace locator no longer exists or is not a directory.
    StaleLocator,
    /// The supplied path is not a Git repository.
    NotRepository,
    /// The path is a repository, but it has no configured remote URL.
    NoRemote,
    /// Exactly one normalized remote identity was found.
    Unique { remote: NormalizedRemote },
    /// More than one distinct usable remote identity was found.
    Ambiguous { remotes: Vec<NormalizedRemote> },
    /// A remote was present, but it was unsafe, malformed, or incomplete.
    Insufficient { reason: InsufficientReason },
}

impl RepositoryLineageEvidence {
    /// Returns the stable identity key only for unique, usable evidence.
    pub fn equality_key(&self) -> Option<String> {
        match self {
            Self::Unique { remote } => Some(remote.equality_key()),
            Self::StaleLocator
            | Self::NotRepository
            | Self::NoRemote
            | Self::Ambiguous { .. }
            | Self::Insufficient { .. } => None,
        }
    }

    /// Whether this result can safely identify the repository lineage.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Unique { .. })
    }
}

/// Errors starting or collecting the local Git probe.
#[derive(Debug, thiserror::Error)]
pub enum RepositoryLineageError {
    #[error("failed to start local git probe: {0}")]
    Spawn(#[source] io::Error),
    #[error("local git probe output exceeded {MAX_CONFIG_OUTPUT_BYTES} bytes")]
    OutputBoundExceeded,
    #[error("local git probe failed with status {0}")]
    CommandFailed(String),
}

/// Normalize one HTTPS, SSH, or scp-like remote without retaining its raw value.
pub fn normalize_remote(raw: &str) -> RemoteNormalization {
    let cleaned: String = raw
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return RemoteNormalization::Invalid;
    }

    let (without_query, stripped_secret) = strip_query_or_fragment(cleaned);
    if stripped_secret {
        return RemoteNormalization::Redacted;
    }

    if let Some(rest) = without_query.strip_prefix("https://") {
        return normalize_url_authority(rest, true);
    }
    if let Some(rest) = without_query.strip_prefix("ssh://") {
        return normalize_url_authority(rest, false);
    }

    normalize_scp_remote(without_query)
}

/// Classify a set of remote URL values deterministically.
pub fn classify_remote_urls<I, S>(remotes: I) -> RepositoryLineageEvidence
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut usable = Vec::new();
    let mut saw_invalid = false;
    let mut saw_redacted = false;

    for raw in remotes {
        match normalize_remote(raw.as_ref()) {
            RemoteNormalization::Usable(remote) => usable.push(remote),
            RemoteNormalization::Redacted => saw_redacted = true,
            RemoteNormalization::Invalid => saw_invalid = true,
        }
    }

    // Unsafe values take precedence over otherwise usable values.  This avoids
    // silently accepting a safe-looking identity from a config containing a
    // second secret-bearing URL.
    if saw_redacted {
        return RepositoryLineageEvidence::Insufficient {
            reason: InsufficientReason::RedactedRemote,
        };
    }
    if saw_invalid {
        return RepositoryLineageEvidence::Insufficient {
            reason: InsufficientReason::InvalidRemote,
        };
    }
    if usable.is_empty() {
        return RepositoryLineageEvidence::NoRemote;
    }

    usable.sort_by_key(NormalizedRemote::equality_key);
    usable.dedup();
    if usable.len() == 1 {
        RepositoryLineageEvidence::Unique {
            remote: usable.remove(0),
        }
    } else {
        RepositoryLineageEvidence::Ambiguous { remotes: usable }
    }
}

/// Inspect one path using only local Git commands.
pub fn inspect_repository_lineage(
    path: impl AsRef<Path>,
) -> Result<RepositoryLineageEvidence, RepositoryLineageError> {
    let path = path.as_ref();
    if !path.is_dir() {
        return Ok(RepositoryLineageEvidence::StaleLocator);
    }
    let repository_probe = run_git(path, &["rev-parse", "--git-dir"])?;
    if !repository_probe.status.success() {
        return Ok(RepositoryLineageEvidence::NotRepository);
    }

    let config_probe = run_git(
        path,
        &["config", "--local", "--get-regexp", r"^remote\..*\.url$"],
    )?;
    if !config_probe.status.success() {
        // `git config --get-regexp` uses status 1 when no key matches.
        if config_probe.status.code() == Some(1) {
            return Ok(RepositoryLineageEvidence::NoRemote);
        }
        return Err(RepositoryLineageError::CommandFailed(status_description(
            &config_probe,
        )));
    }

    let (remotes, malformed) = parse_remote_config(&config_probe.stdout);
    if malformed {
        return Ok(RepositoryLineageEvidence::Insufficient {
            reason: InsufficientReason::MalformedConfig,
        });
    }
    Ok(classify_remote_urls(remotes))
}

fn normalize_url_authority(rest: &str, https: bool) -> RemoteNormalization {
    let Some(path_start) = rest.find('/') else {
        return RemoteNormalization::Invalid;
    };
    let authority = &rest[..path_start];
    let path = &rest[path_start..];
    if authority.is_empty() || authority.chars().any(char::is_whitespace) {
        return RemoteNormalization::Invalid;
    }

    let authority = if let Some(at) = authority.rfind('@') {
        let user_info = &authority[..at];
        if https || user_info.is_empty() || user_info.contains(':') {
            return RemoteNormalization::Redacted;
        }
        &authority[at + 1..]
    } else {
        authority
    };
    let Some(host) = normalize_host(authority) else {
        return RemoteNormalization::Invalid;
    };
    normalize_remote_parts(host, path)
}

fn normalize_scp_remote(raw: &str) -> RemoteNormalization {
    let Some(colon) = raw.find(':') else {
        return RemoteNormalization::Invalid;
    };
    if raw[..colon].contains('/') || raw[..colon].is_empty() {
        return RemoteNormalization::Invalid;
    }
    let host_part = &raw[..colon];
    let host_part = if let Some(at) = host_part.rfind('@') {
        let user_info = &host_part[..at];
        if user_info.is_empty() || user_info.contains(':') {
            return RemoteNormalization::Redacted;
        }
        &host_part[at + 1..]
    } else {
        host_part
    };
    let Some(host) = normalize_host(host_part) else {
        return RemoteNormalization::Invalid;
    };
    normalize_remote_parts(host, &raw[colon + 1..])
}

fn normalize_host(authority: &str) -> Option<String> {
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return None;
    }
    if authority.starts_with('[') {
        let close = authority.find(']')?;
        let suffix = &authority[close + 1..];
        if !suffix.is_empty() && (!suffix.starts_with(':') || suffix[1..].is_empty()) {
            return None;
        }
        if suffix.starts_with(':') && !suffix[1..].chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        return Some(authority.to_ascii_lowercase());
    }

    let colon_count = authority.chars().filter(|&c| c == ':').count();
    if colon_count > 1 {
        return None;
    }
    if let Some(colon) = authority.find(':') {
        if authority[colon + 1..].is_empty()
            || !authority[colon + 1..].chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
    }
    if authority.chars().any(|c| c.is_whitespace() || c == '/') {
        return None;
    }
    Some(authority.to_ascii_lowercase())
}

fn normalize_remote_parts(host: String, raw_path: &str) -> RemoteNormalization {
    let mut path = raw_path.trim_matches('/');
    if path.ends_with(".git") {
        path = &path[..path.len() - 4];
    }
    path = path.trim_matches('/');
    if path.is_empty()
        || path.contains('\\')
        || path.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return RemoteNormalization::Invalid;
    }
    RemoteNormalization::Usable(NormalizedRemote {
        host,
        path: path.to_owned(),
    })
}

fn strip_query_or_fragment(value: &str) -> (&str, bool) {
    let query = value.find('?');
    let fragment = value.find('#');
    let split_at = match (query, fragment) {
        (Some(query), Some(fragment)) => Some(query.min(fragment)),
        (Some(query), None) => Some(query),
        (None, Some(fragment)) => Some(fragment),
        (None, None) => None,
    };
    let Some(split_at) = split_at else {
        return (value, false);
    };
    let suffix = &value[split_at + 1..];
    (value[..split_at].trim_end(), contains_secret_marker(suffix))
}

fn contains_secret_marker(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "token=",
        "access_token=",
        "password=",
        "passwd=",
        "secret=",
        "api_key=",
        "apikey=",
        "auth=",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn parse_remote_config(output: &[u8]) -> (Vec<String>, bool) {
    let Ok(output) = std::str::from_utf8(output) else {
        return (Vec::new(), true);
    };
    let mut remotes = Vec::new();
    let mut malformed = false;
    for line in output.lines() {
        let Some(separator) = line.find(char::is_whitespace) else {
            malformed = true;
            continue;
        };
        let key = &line[..separator];
        let value = line[separator..].trim();
        if key.starts_with("remote.") && key.ends_with(".url") {
            if value.is_empty() {
                malformed = true;
            } else {
                remotes.push(value.to_owned());
            }
        }
    }
    (remotes, malformed)
}

fn run_git(path: &Path, args: &[&str]) -> Result<Output, RepositoryLineageError> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(RepositoryLineageError::Spawn)?;

    let stdout = child.stdout.take().expect("stdout was configured as piped");
    let mut bytes = Vec::with_capacity(MAX_CONFIG_OUTPUT_BYTES.min(4096));
    stdout
        .take((MAX_CONFIG_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(RepositoryLineageError::Spawn)?;
    if bytes.len() > MAX_CONFIG_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(RepositoryLineageError::OutputBoundExceeded);
    }
    let status = child.wait().map_err(RepositoryLineageError::Spawn)?;
    Ok(Output {
        status,
        stdout: bytes,
        stderr: Vec::new(),
    })
}

fn status_description(output: &Output) -> String {
    output.status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| code.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn normalizes_supported_remote_shapes() {
        let cases = [
            (
                "https://github.com/dbowm91/codegg.git/",
                "github.com/dbowm91/codegg",
            ),
            (
                "ssh://git@GITHUB.com:22/dbowm91/codegg.git?ref=main#readme",
                "github.com:22/dbowm91/codegg",
            ),
            (
                "git@github.com:dbowm91/codegg.git",
                "github.com/dbowm91/codegg",
            ),
        ];
        for (raw, expected) in cases {
            let RemoteNormalization::Usable(remote) = normalize_remote(raw) else {
                panic!("expected usable remote for {raw}");
            };
            assert_eq!(remote.equality_key(), format!("git:{expected}"));
        }
    }

    #[test]
    fn strips_controls_and_rejects_secret_material_without_retaining_it() {
        let normalized = normalize_remote("https://user:super-secret@example.com/team/repo.git\n");
        assert_eq!(normalized, RemoteNormalization::Redacted);
        assert!(!format!("{normalized:?}").contains("super-secret"));

        assert_eq!(
            normalize_remote("https://example.com/team/repo?access_token=secret"),
            RemoteNormalization::Redacted
        );
        let RemoteNormalization::Usable(remote) =
            normalize_remote("https://example.com/\tteam/repo.git")
        else {
            panic!("expected usable control-stripped remote");
        };
        assert_eq!(remote.equality_key(), "git:example.com/team/repo");
    }

    #[test]
    fn classification_is_deterministic_and_path_independent() {
        let first = classify_remote_urls([
            "git@EXAMPLE.com:team/repo.git",
            "ssh://git@example.com/team/repo",
        ]);
        let second = classify_remote_urls([
            "ssh://git@example.com/team/repo",
            "git@EXAMPLE.com:team/repo.git",
        ]);
        assert_eq!(first, second);
        assert_eq!(
            first.equality_key().as_deref(),
            Some("git:example.com/team/repo")
        );

        let ambiguous = classify_remote_urls(["https://one.example/a", "https://two.example/a"]);
        assert!(matches!(
            ambiguous,
            RepositoryLineageEvidence::Ambiguous { .. }
        ));
        assert_eq!(ambiguous.equality_key(), None);
    }

    #[test]
    fn local_probe_distinguishes_repository_without_remote() {
        let directory = tempfile::tempdir().expect("tempdir");
        let initialized = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(directory.path())
            .status()
            .expect("git is installed");
        if !initialized.success() {
            return;
        }
        assert_eq!(
            inspect_repository_lineage(directory.path()).expect("local probe"),
            RepositoryLineageEvidence::NoRemote
        );

        let outside = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            inspect_repository_lineage(outside.path()).expect("local probe"),
            RepositoryLineageEvidence::NotRepository
        );

        let missing = outside.path().join("removed-workspace");
        assert_eq!(
            inspect_repository_lineage(&missing).expect("stale locator probe"),
            RepositoryLineageEvidence::StaleLocator
        );
    }
}
