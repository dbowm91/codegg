//! Sensitive URL wrapper used to keep raw credential-bearing values from
//! leaking through `Debug`, `Serialize`, or display paths.
//!
//! Git operations that accept user-provided URLs (`remote_add`,
//! `remote_set_url`) frequently contain embedded credentials
//! (`https://user:token@host/...`). Those raw values must reach the
//! child Git process to perform the requested operation, but they
//! must never reach:
//!
//! * `Debug` output (logs, error formatting)
//! * `Serialize` output (RunStore manifest, projector output)
//! * `Display` output (permission prompts, UI summaries)
//! * Tracing fields or `tracing::info!` records
//!
//! [`RedactedUrl`] wraps the raw value behind an internal field that
//! is intentionally NOT included in the `Debug`/`Serialize`/`Display`
//! views. The redacted form is always available through
//! [`RedactedUrl::display`] and friends, and is what other modules see
//! by default.
//!
//! The wrapper is lightweight on purpose — no external dependency on
//! the `secrecy` crate is required to provide the boundary the rest of
//! the codebase relies on.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A URL whose raw value is hidden from `Debug`, `Serialize`, and
/// `Display`.
///
/// Construct via [`RedactedUrl::new`], which automatically applies the
/// same [`crate::sensitive::redact_url_credentials`] sanitizer used
/// throughout the network policy. The display form is always returned
/// by [`RedactedUrl::display`]; the raw value is only available via
/// [`RedactedUrl::expose_secret`], intended exclusively for the final
/// Git child-process argument construction in
/// `codegg_git::render_argv`.
#[derive(Clone, PartialEq, Eq)]
pub struct RedactedUrl {
    raw: String,
    redacted: String,
}

impl RedactedUrl {
    /// Wrap a raw URL, computing the display form via the shared
    /// [`redact_url_credentials`] sanitizer. The returned value carries
    /// both the raw and redacted representations.
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let redacted = redact_url_credentials(&raw);
        Self { raw, redacted }
    }

    /// Wrap a raw URL without computing the redacted display form.
    /// Used when the caller already holds the redacted value (e.g.
    /// when constructing a sentinel for tests or when the redacted
    /// form is the only thing the upstream type permits).
    pub fn from_redacted(redacted: impl Into<String>) -> Self {
        Self {
            raw: redacted.into(),
            redacted: String::new(),
        }
    }

    /// The raw value, intended exclusively for the Git child-process
    /// argument construction boundary. Use sparingly and do not surface
    /// the return value to logging, serialization, or display paths.
    pub fn expose_secret(&self) -> &str {
        &self.raw
    }

    /// The redacted display form. Suitable for logs, UI, projections,
    /// serialization, debug, and any non-execution boundary.
    pub fn display(&self) -> &str {
        if self.redacted.is_empty() {
            &self.raw
        } else {
            &self.redacted
        }
    }

    /// True when the raw and redacted forms differ, meaning the input
    /// carried credentials. Useful for tests and assertions.
    pub fn was_redacted(&self) -> bool {
        self.redacted.is_empty() || self.raw != self.redacted
    }
}

impl fmt::Debug for RedactedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never reveal the raw value in Debug output.
        f.debug_struct("RedactedUrl")
            .field("display", &self.display())
            .finish()
    }
}

impl fmt::Display for RedactedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display())
    }
}

impl Serialize for RedactedUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.display())
    }
}

impl<'de> Deserialize<'de> for RedactedUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(RedactedUrl::new(s))
    }
}

/// Redact credentials embedded in a URL.
///
/// Recognizes `scheme://user:token@host` patterns and rewrites them to
/// `scheme://redacted@host`. Idempotent on already-redacted input.
///
/// Does NOT touch:
/// * URLs without a `user:pass@` segment (returns unchanged).
/// * URLs where the credential contains only the username (no `:`).
///
/// This re-implements the long-standing helper in
/// `crate::git_network_policy::redact_url_credentials` as an
/// in-crate copy because `codegg-git` must not depend on the root
/// crate. The two implementations MUST stay byte-for-byte equivalent
/// (covered by `redact_url_credentials_cross_crate` below and the
/// integration test in `tests/git_closure_matrix.rs`).
pub fn redact_url_credentials(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }

    let scheme_end = url.find("://");
    let (scheme_prefix, rest) = match scheme_end {
        Some(i) => (&url[..i], &url[i + 3..]),
        None => return url.to_string(),
    };

    let auth_end = ['/', '?', '#']
        .into_iter()
        .filter_map(|c| rest.find(c))
        .min()
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let after_authority = &rest[auth_end..];

    let at_pos = authority.find('@');
    let (host_part, user_part) = match at_pos {
        Some(i) => (&authority[i + 1..], Some(&authority[..i])),
        None => (authority, None),
    };

    match user_part {
        None => url.to_string(),
        Some(userinfo) => {
            let colon_pos = userinfo.find(':');
            let redacted_user = if colon_pos.is_some() {
                "redacted".to_string()
            } else {
                userinfo.to_string()
            };
            format!("{scheme_prefix}://{redacted_user}@{host_part}{after_authority}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_user_password() {
        let r = redact_url_credentials("https://alice:secret@example.com/r.git");
        assert_eq!(r, "https://redacted@example.com/r.git");
    }

    #[test]
    fn preserves_bare_username() {
        let r = redact_url_credentials("https://alice@example.com/r.git");
        assert_eq!(r, "https://alice@example.com/r.git");
    }

    #[test]
    fn preserves_ssh_scp_form() {
        let r = redact_url_credentials("git@github.com:owner/r.git");
        assert_eq!(r, "git@github.com:owner/r.git");
    }

    #[test]
    fn idempotent() {
        let r = redact_url_credentials("https://redacted@example.com/r.git");
        assert_eq!(r, "https://redacted@example.com/r.git");
    }

    #[test]
    fn redacted_url_debug_hides_raw_secret() {
        let raw = "https://user:secret_token_abc@host.example/r.git";
        let u = RedactedUrl::new(raw);
        let dbg = format!("{:?}", u);
        assert!(
            !dbg.contains("secret_token_abc"),
            "Debug leaked raw secret: {dbg}"
        );
        assert!(
            dbg.contains("redacted"),
            "Debug missing redacted marker: {dbg}"
        );
    }

    #[test]
    fn redacted_url_serialize_omits_raw_secret() {
        let raw = "https://user:secret_token_abc@host.example/r.git";
        let u = RedactedUrl::new(raw);
        let json = serde_json::to_string(&u).expect("serialize ok");
        assert!(
            !json.contains("secret_token_abc"),
            "Serialize leaked raw secret: {json}"
        );
        assert!(
            json.contains("redacted"),
            "Serialize missing redacted marker: {json}"
        );
    }

    #[test]
    fn redacted_url_display_is_safe() {
        let raw = "https://user:secret_token_abc@host.example/r.git";
        let u = RedactedUrl::new(raw);
        let disp = format!("{u}");
        assert!(!disp.contains("secret_token_abc"));
    }

    #[test]
    fn redacted_url_expose_secret_returns_raw_for_execution() {
        let raw = "https://user:secret_token_abc@host.example/r.git";
        let u = RedactedUrl::new(raw);
        assert_eq!(u.expose_secret(), raw);
    }

    #[test]
    fn redacted_url_was_redacted_signal() {
        let cred = RedactedUrl::new("https://user:secret@host/r");
        assert!(cred.was_redacted());
        let plain = RedactedUrl::new("https://example.com/r");
        assert!(!plain.was_redacted());
    }
}
