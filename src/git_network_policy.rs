//! Git network execution policy.
//!
//! Defines the env-policy, redaction, and permission classification
//! shared by fetch, pull, push, and remote-config mutations.
//!
//! Network operations are distinguished from local mutations by:
//!
//! * Hard-pinned `GIT_TERMINAL_PROMPT=0` so credential helpers cannot
//!   block (carried over from `GitEnvPolicy`).
//! * Additional redaction of credentials embedded in URLs (e.g.,
//!   `https://user:token@host/...`) before persistence or display.
//! * A `NetworkPolicy` that classifies transport failures into
//!   DNS / connect / auth / ref-rejection / timeout categories so the
//!   TUI and projector can present actionable diagnostics.
//!
//! Environment variables intentionally preserved for network
//! operations (so the user's credential helper / SSH agent work):
//!
//! * `GIT_ASKPASS`, `GIT_TERMINAL_PROMPT` (pinned to `0`)
//! * `SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `SSH_*_PROXY`
//! * `HOME`, `USERPROFILE`
//! * `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`
//! * `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`
//! * `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`
//! * `XDG_CONFIG_HOME`, `XDG_DATA_HOME`
//!
//! What is intentionally cleared:
//!
//! * `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, `EDITOR`, `VISUAL` — to
//!   prevent interactive editor prompts on push commit-message edits.
//! * `GIT_PAGER`, `PAGER` — no paginated output for non-interactive
//!   execution.
//!
//! All env var names are stored as `&'static str` so the policy
//! itself has no allocations.

use std::time::Duration;

/// Environment variables preserved for network operations, on top of
/// the always-restored set in [`crate::git_mutations::ALLOWED_ENV_VARS`].
pub const NETWORK_ALLOWED_ENV_VARS: &[&str] = &[
    "GIT_ASKPASS",
    "GIT_SSH_COMMAND",
    "GIT_SSH_VARIANT",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "GIT_TRACE",
    "GIT_TRACE_PACKET",
    "GIT_CURL_VERBOSE",
];

/// Per-operation timeout for network operations. Network operations
/// carry a tighter default than local mutations because failures are
/// often slow (TCP retransmits, DNS retries).
pub const NETWORK_DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Classification of network failure modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkFailureKind {
    /// DNS resolution failed (host not found).
    Dns,
    /// TCP/TLS connect failed.
    Connect,
    /// Authentication failed (HTTP 401/403, SSH handshake refused).
    Authentication,
    /// Authorization failed (HTTP 407 proxy auth, or SSH publickey
    /// refused).
    Authorization,
    /// Remote refused the ref update (non-fast-forward, protected
    /// branch).
    RefRejected,
    /// Operation exceeded timeout.
    Timeout,
    /// Unclassified transport error.
    Transport,
}

/// Classify the stderr of a network operation into a failure kind.
///
/// The classifier is heuristic: it inspects common Git transport
/// diagnostics. It returns `None` when no failure mode is recognizable
/// (caller should treat as `Transport`).
pub fn classify_network_failure(
    stderr: &str,
    exit_code: i32,
    timed_out: bool,
) -> NetworkFailureKind {
    if timed_out {
        return NetworkFailureKind::Timeout;
    }
    if exit_code == 0 {
        return NetworkFailureKind::Transport; // success path
    }
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("could not resolve host")
        || lower.contains("getaddrinfo")
        || lower.contains("name or service not known")
    {
        return NetworkFailureKind::Dns;
    }
    if lower.contains("connection timed out")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("failed to connect")
        || lower.contains("could not connect")
        || lower.contains("ssl connect")
        || lower.contains("tls")
    {
        return NetworkFailureKind::Connect;
    }
    if lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("could not read password")
        || lower.contains("invalid username or password")
        || lower.contains("permission denied (publickey)")
        || lower.contains("unsupported authentication method")
        || lower.contains("remote: invalid username or password")
    {
        return NetworkFailureKind::Authentication;
    }
    if lower.contains("http 407")
        || lower.contains("proxy authentication")
        || lower.contains("proxy: auth")
    {
        return NetworkFailureKind::Authorization;
    }
    if lower.contains("non-fast-forward")
        || lower.contains("failed to push some refs")
        || lower.contains("[remote rejected]")
        || lower.contains("protected branch")
        || lower.contains("deny updating")
    {
        return NetworkFailureKind::RefRejected;
    }
    NetworkFailureKind::Transport
}

/// Redact credentials embedded in a URL.
///
/// Recognizes `scheme://user:token@host` patterns and rewrites them to
/// `scheme://redacted@host`. Idempotent on already-redacted input.
///
/// Does NOT touch:
/// * URLs without a `user:pass@` segment (returns unchanged).
/// * URLs where the credential contains only the username (no `:`).
pub fn redact_url_credentials(url: &str) -> String {
    // Quick reject: no `://` separator means no scheme; the regex still
    // works but is unnecessary. Most common case: scp-style URLs like
    // `git@github.com:foo/bar.git` — these are username-only and left
    // alone.
    if url.is_empty() {
        return String::new();
    }

    let scheme_end = url.find("://");
    let (scheme_prefix, rest) = match scheme_end {
        Some(i) => (&url[..i], &url[i + 3..]),
        None => return url.to_string(),
    };

    // Find the end of the authority component — i.e. the next `/`,
    // `?`, `#`, or end of string.
    let auth_end = ['/', '?', '#']
        .into_iter()
        .filter_map(|c| rest.find(c))
        .min()
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let after_authority = &rest[auth_end..];

    // Authority is `[user[:password]@]host[:port]`. Find the `@`.
    let at_pos = authority.find('@');
    let (host_part, user_part) = match at_pos {
        Some(i) => (&authority[i + 1..], Some(&authority[..i])),
        None => (authority, None),
    };

    match user_part {
        None => url.to_string(),
        Some(userinfo) => {
            // userinfo can be just "user" or "user:password". Redact
            // both forms.
            let colon_pos = userinfo.find(':');
            let redacted_user = if colon_pos.is_some() {
                // user:password → redact entirely
                "redacted".to_string()
            } else {
                // bare user — leave intact (often SSH-key derived)
                userinfo.to_string()
            };
            format!("{scheme_prefix}://{redacted_user}@{host_part}{after_authority}")
        }
    }
}

/// Sanitize a list of URLs in place, returning a new Vec with credentials
/// redacted.
pub fn redact_url_list<I, S>(urls: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    urls.into_iter()
        .map(|s| redact_url_credentials(s.as_ref()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_https_user_password() {
        let r = redact_url_credentials("https://alice:secret@example.com/repo.git");
        assert_eq!(r, "https://redacted@example.com/repo.git");
        assert!(!r.contains("secret"));
    }

    #[test]
    fn redact_https_user_only() {
        let r = redact_url_credentials("https://alice@example.com/repo.git");
        // bare user left intact (commonly SSH key derivation)
        assert_eq!(r, "https://alice@example.com/repo.git");
    }

    #[test]
    fn redact_http_with_port() {
        let r = redact_url_credentials("http://u:p@host.example.com:8080/r.git");
        assert_eq!(r, "http://redacted@host.example.com:8080/r.git");
    }

    #[test]
    fn redact_ssh_scp_form_unchanged() {
        let r = redact_url_credentials("git@github.com:owner/repo.git");
        assert_eq!(r, "git@github.com:owner/repo.git");
    }

    #[test]
    fn no_credentials_passthrough() {
        let r = redact_url_credentials("https://github.com/owner/repo.git");
        assert_eq!(r, "https://github.com/owner/repo.git");
    }

    #[test]
    fn redact_empty_returns_empty() {
        let r = redact_url_credentials("");
        assert_eq!(r, "");
    }

    #[test]
    fn redact_already_redacted_idempotent() {
        let r = redact_url_credentials("https://redacted@example.com/repo.git");
        assert_eq!(r, "https://redacted@example.com/repo.git");
    }

    #[test]
    fn redact_token_in_url() {
        let r = redact_url_credentials("https://x-access-token:ghp_abc123@github.com/r.git");
        assert!(!r.contains("ghp_abc123"));
        assert!(r.contains("redacted"));
    }

    #[test]
    fn redact_list() {
        let r = redact_url_list([
            "https://u:p@a.com/r.git",
            "ssh://git@b.com/r.git",
            "https://plain.com/r.git",
        ]);
        assert_eq!(
            r,
            vec![
                "https://redacted@a.com/r.git".to_string(),
                "ssh://git@b.com/r.git".to_string(),
                "https://plain.com/r.git".to_string(),
            ]
        );
    }

    #[test]
    fn classify_dns_failure() {
        let kind = classify_network_failure(
            "fatal: unable to access: Could not resolve host: github.com",
            128,
            false,
        );
        assert_eq!(kind, NetworkFailureKind::Dns);
    }

    #[test]
    fn classify_connect_failure() {
        let kind =
            classify_network_failure("fatal: unable to connect: Connection refused", 128, false);
        assert_eq!(kind, NetworkFailureKind::Connect);
    }

    #[test]
    fn classify_authentication_failure() {
        let kind = classify_network_failure(
            "remote: Invalid username or password.\nfatal: Authentication failed",
            128,
            false,
        );
        assert_eq!(kind, NetworkFailureKind::Authentication);
    }

    #[test]
    fn classify_ssh_publickey_failure() {
        let kind =
            classify_network_failure("git@github.com: Permission denied (publickey).", 128, false);
        assert_eq!(kind, NetworkFailureKind::Authentication);
    }

    #[test]
    fn classify_non_fast_forward() {
        let kind = classify_network_failure(
            "To github.com:foo/bar.git\n ! [rejected] main -> main (non-fast-forward)",
            1,
            false,
        );
        assert_eq!(kind, NetworkFailureKind::RefRejected);
    }

    #[test]
    fn classify_timeout_overrides_stderr() {
        let kind = classify_network_failure("anything", -1, true);
        assert_eq!(kind, NetworkFailureKind::Timeout);
    }

    #[test]
    fn classify_unrecognized_falls_back_to_transport() {
        let kind = classify_network_failure("fatal: some unknown error", 128, false);
        assert_eq!(kind, NetworkFailureKind::Transport);
    }
}
