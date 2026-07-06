//! Shell output redaction pipeline (Phase 8).
//!
//! Deterministic, regex-based secret filtering for command output projected
//! to model context. The [`Redactor`] applies a fixed set of compiled
//! [`RedactRule`] implementations in a defined order and produces a
//! [`RedactedOutput`] containing the filtered text and metadata about what
//! was replaced.
//!
//! Rules are **deterministic** — no ML, no heuristics. Original sensitive
//! values are never logged or exposed. Replacement markers follow the
//! pattern `[REDACTED:<rule-class>]`.

use std::sync::LazyLock;

use regex::Regex;

/// Output of the redaction pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedOutput {
    /// The redacted text with sensitive values replaced by markers.
    pub text: String,
    /// Total number of replacements applied across all rules.
    pub replacements: usize,
    /// Names of rules that produced at least one replacement, in application order.
    pub applied_rules: Vec<String>,
}

/// A single redaction rule.
///
/// Each implementation compiles its own regex patterns and replaces matches
/// with a stable marker. Rules must be **false-positive resistant**: ordinary
/// compiler diagnostics containing words like "token", "key", or "secret"
/// should NOT be redacted.
pub trait RedactRule {
    /// Human-readable name for this rule class (used in markers and metadata).
    fn name(&self) -> &str;

    /// Apply this rule to `text`, returning the redacted text and the number
    /// of replacements made.
    fn redact(&self, text: &str) -> (String, usize);
}

// ---------------------------------------------------------------------------
// Rule implementations
// ---------------------------------------------------------------------------

/// Authorization headers and inline access tokens.
///
/// Matches:
/// - `Authorization: Bearer <token>`
/// - `Authorization: Basic <credentials>`
/// - `api[_-]?key[=:]\s*["']?<value>["']?` (case-insensitive)
/// - `X-Api-Key: <value>`
struct AuthorizationRule;

impl RedactRule for AuthorizationRule {
    fn name(&self) -> &str {
        "api-key"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;
        let mut out = BEARER_RE
            .replace_all(text, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:bearer-token]", &caps[1])
            })
            .into_owned();

        out = BASIC_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:basic-creds]", &caps[1])
            })
            .into_owned();

        out = API_KEY_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                let prefix = &caps[1]; // e.g. "api_key=" or "X-Api-Key: "
                let value = &caps[2];
                if value.len() <= 4 {
                    // Too short to meaningfully redact — skip to avoid false positives
                    caps[0].to_string()
                } else {
                    format!("{prefix}[REDACTED:api-key]")
                }
            })
            .into_owned();

        (out, count)
    }
}

/// Environment-style secret assignments.
///
/// Matches `SECRET=...`, `API_TOKEN=...`, `PASSWORD=...`, etc. where the
/// variable name contains a sensitive keyword and the value is a non-empty
/// string (typically quoted or unquoted up to whitespace/end-of-line).
struct EnvSecretRule;

impl RedactRule for EnvSecretRule {
    fn name(&self) -> &str {
        "env-secret"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;
        let out = ENV_SECRET_RE
            .replace_all(text, |caps: &regex::Captures| {
                let var_name = &caps[1];
                let value = &caps[2];
                if value.len() <= 4 {
                    // Short values are likely placeholders — don't redact
                    caps[0].to_string()
                } else {
                    count += 1;
                    format!("{var_name}=[REDACTED:env-secret]")
                }
            })
            .into_owned();
        (out, count)
    }
}

/// PEM-encoded private keys and certificates.
///
/// Matches `-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----` and
/// `-----BEGIN CERTIFICATE-----` blocks through to their corresponding
/// END markers.
struct PemBlockRule;

impl RedactRule for PemBlockRule {
    fn name(&self) -> &str {
        "pem-key"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;
        let out = PEM_RE
            .replace_all(text, |caps: &regex::Captures| {
                count += 1;
                let begin_line = &caps[1];
                let end_line = &caps[2];
                format!("{begin_line}\n[REDACTED:pem-block]\n{end_line}")
            })
            .into_owned();
        (out, count)
    }
}

/// Cloud and service-account credentials.
///
/// Matches:
/// - AWS access key IDs (`AKIA...`) and secret access keys
/// - GCP service account JSON fields (`"private_key": "..."`)
/// - Azure connection strings and storage keys
struct CloudCredentialRule;

impl RedactRule for CloudCredentialRule {
    fn name(&self) -> &str {
        "cloud-cred"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;

        // AWS access key ID
        let out = AWS_ACCESS_KEY_RE
            .replace_all(text, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:aws-access-key]{}", &caps[1], &caps[3])
            })
            .into_owned();

        // AWS secret access key (after common context like "aws_secret_access_key")
        let out = AWS_SECRET_KEY_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:aws-secret-key]", &caps[1])
            })
            .into_owned();

        // GCP private_key field in service account JSON
        let out = GCP_PRIVATE_KEY_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:gcp-private-key]", &caps[1])
            })
            .into_owned();

        // Azure connection strings
        let out = AZURE_CONN_STR_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:azure-conn-str]", &caps[1])
            })
            .into_owned();

        (out, count)
    }
}

/// URLs with embedded credentials.
///
/// Matches `https://user:password@host` or `http://user:pass@host` patterns.
struct EmbeddedCredentialUrlRule;

impl RedactRule for EmbeddedCredentialUrlRule {
    fn name(&self) -> &str {
        "embedded-cred-url"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;
        let out = EMBEDDED_CRED_URL_RE
            .replace_all(text, |caps: &regex::Captures| {
                count += 1;
                format!("{}{}[REDACTED:embedded-cred]@", &caps[1], &caps[2])
            })
            .into_owned();
        (out, count)
    }
}

/// Cookies and session material.
///
/// Matches:
/// - `Cookie: <value>` headers
/// - `Set-Cookie: <value>` headers
/// - `session_id=...` or `sid=...` query parameters or assignments
/// - `csrf_token=...` or `_csrf=...` patterns
struct SessionMaterialRule;

impl RedactRule for SessionMaterialRule {
    fn name(&self) -> &str {
        "session-material"
    }

    fn redact(&self, text: &str) -> (String, usize) {
        let mut count = 0usize;

        // Cookie / Set-Cookie headers
        let out = COOKIE_HEADER_RE
            .replace_all(text, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:cookie]", &caps[1])
            })
            .into_owned();

        // session_id / sid assignments
        let out = SESSION_ID_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:session-id]", &caps[1])
            })
            .into_owned();

        // CSRF tokens
        let out = CSRF_TOKEN_RE
            .replace_all(&out, |caps: &regex::Captures| {
                count += 1;
                format!("{}[REDACTED:csrf-token]", &caps[1])
            })
            .into_owned();

        (out, count)
    }
}

// ---------------------------------------------------------------------------
// Compiled regex patterns
// ---------------------------------------------------------------------------

static BEARER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(Authorization:\s*Bearer\s+)(\S{8,})"#).expect("valid bearer regex")
});

static BASIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(Authorization:\s*Basic\s+)(\S{8,})"#).expect("valid basic regex")
});

static API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)((?:api[_-]?key|X-Api-Key)[=:]\s*["']?)([^"'\s;,)]{5,})"#)
        .expect("valid api-key regex")
});

/// Matches `VAR_NAME=value` where VAR_NAME is ALL UPPERCASE and contains a
/// sensitive keyword. The uppercase requirement prevents false positives on
/// compiler diagnostics like `secret_key = String::new()`.
static ENV_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\b(?:[A-Z][A-Z0-9_]*?(?:SECRET|KEY|TOKEN|PASSWORD|CREDENTIAL|PRIV(?:ATE)?_?KEY|AUTH_TOKEN|ACCESS_KEY|SECRET_KEY)[A-Z0-9_]*))\s*[=:]\s*["']?([^"'\s;,}\n]{5,})"#)
        .expect("valid env-secret regex")
});

/// Matches a PEM block from BEGIN to END.
/// `(?s)` enables dotall mode so `.` matches newlines.
/// Capture group 1 = BEGIN line, capture group 2 = END line.
static PEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)(-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----).*?(-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----)"#)
        .expect("valid pem private key regex")
});

/// AWS access key ID: 4 uppercase chars + 16 alphanumeric.
static AWS_ACCESS_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([\s"']|^)(AKIA[0-9A-Z]{16})([\s"']|$)"#).expect("valid aws access key regex")
});

/// AWS secret access key in assignment context.
static AWS_SECRET_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(aws_secret_access_key\s*[=:]\s*["']?)([^"'\s;,)]{20,})"#)
        .expect("valid aws secret key regex")
});

/// GCP private_key field in JSON.
static GCP_PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("private_key"\s*:\s*")([^"]{20,})""#).expect("valid gcp private key regex")
});

/// Azure connection string.
static AZURE_CONN_STR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(DefaultEndpointsProtocol=https?;AccountName=[^;]+;AccountKey=)([A-Za-z0-9+/=]{20,})"#)
        .expect("valid azure conn str regex")
});

/// URL with embedded credentials: `scheme://user:pass@host`.
static EMBEDDED_CRED_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(https?://)([^@\s]{1,64}):([^@\s]{4,128})@"#)
        .expect("valid embedded cred url regex")
});

/// Cookie / Set-Cookie header values.
static COOKIE_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)((?:Set-)?Cookie:\s*)(\S+)"#).expect("valid cookie header regex")
});

/// session_id or sid assignment.
static SESSION_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)((?:session[_-]?id|sid)\s*[=:]\s*["']?)([A-Za-z0-9_-]{16,})"#)
        .expect("valid session id regex")
});

/// csrf_token or _csrf assignment.
static CSRF_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)((?:csrf[_-]?token|_csrf)\s*[=:]\s*["']?)([A-Za-z0-9_-]{16,})"#)
        .expect("valid csrf token regex")
});

// ---------------------------------------------------------------------------
// Redactor
// ---------------------------------------------------------------------------

/// Stateless redactor that applies a fixed set of compiled regex rules.
///
/// Construct via [`Redactor::new`] (which compiles all patterns) or use the
/// [`Redactor::default`] singleton. After construction, the redactor is
/// immutable and safe to share across threads.
pub struct Redactor {
    rules: Vec<Box<dyn RedactRule + Send + Sync>>,
}

impl Redactor {
    /// Create a new redactor with all built-in rules.
    pub fn new() -> Self {
        Self {
            rules: vec![
                Box::new(AuthorizationRule),
                Box::new(EnvSecretRule),
                Box::new(PemBlockRule),
                Box::new(CloudCredentialRule),
                Box::new(EmbeddedCredentialUrlRule),
                Box::new(SessionMaterialRule),
            ],
        }
    }

    /// Apply all redaction rules to `text` in defined order.
    ///
    /// Returns a [`RedactedOutput`] containing the filtered text, the total
    /// number of replacements, and the names of rules that matched.
    pub fn redact(&self, text: &str) -> RedactedOutput {
        if text.is_empty() {
            return RedactedOutput {
                text: String::new(),
                replacements: 0,
                applied_rules: Vec::new(),
            };
        }

        let mut current = text.to_owned();
        let mut total_replacements = 0usize;
        let mut applied_rules = Vec::new();

        for rule in &self.rules {
            let (redacted, count) = rule.redact(&current);
            if count > 0 {
                total_replacements += count;
                applied_rules.push(rule.name().to_owned());
            }
            current = redacted;
        }

        RedactedOutput {
            text: current,
            replacements: total_replacements,
            applied_rules,
        }
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_empty_output() {
        let redactor = Redactor::new();
        let result = redactor.redact("");
        assert_eq!(result.text, "");
        assert_eq!(result.replacements, 0);
        assert!(result.applied_rules.is_empty());
    }

    #[test]
    fn no_secrets_pass_through_unchanged() {
        let redactor = Redactor::new();
        let input = "Compiling crate foo v0.1.0\nwarning: unused variable `token`\nerror[E0596]: cannot borrow `key` as mutable";
        let result = redactor.redact(input);
        assert_eq!(result.text, input);
        assert_eq!(result.replacements, 0);
    }

    #[test]
    fn bearer_token_is_redacted() {
        let redactor = Redactor::new();
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:bearer-token]"));
        assert!(!result.text.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(result.applied_rules.contains(&"api-key".to_owned()));
        assert!(result.replacements >= 1);
    }

    #[test]
    fn basic_auth_is_redacted() {
        let redactor = Redactor::new();
        let input = "Authorization: Basic dXNlcm5hbWU6cGFzc3dvcmQxMjM0NQ==";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:basic-creds]"));
        assert!(!result.text.contains("dXNlcm5hbWU6cGFzc3dvcmQxMjM0NQ=="));
    }

    #[test]
    fn api_key_assignment_is_redacted() {
        let redactor = Redactor::new();
        let input = "api_key=sk-proj-abcdefghijklmnopqrstuvwxyz123456";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:api-key]"));
        assert!(!result
            .text
            .contains("sk-proj-abcdefghijklmnopqrstuvwxyz123456"));
    }

    #[test]
    fn api_key_in_quotes_is_redacted() {
        let redactor = Redactor::new();
        let input = r#"X-Api-Key: "super-secret-api-key-value-1234""#;
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:api-key]"));
    }

    #[test]
    fn env_secret_password_is_redacted() {
        let redactor = Redactor::new();
        let input = "DATABASE_PASSWORD=sup3rS3cretP@ssw0rd!";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:env-secret]"));
        assert!(!result.text.contains("sup3rS3cretP@ssw0rd!"));
    }

    #[test]
    fn env_secret_api_token_is_redacted() {
        let redactor = Redactor::new();
        let input = "export CI_API_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:env-secret]"));
    }

    #[test]
    fn short_env_values_not_redacted() {
        let redactor = Redactor::new();
        let input = "TOKEN=abc";
        let result = redactor.redact(input);
        assert_eq!(result.text, input);
        assert_eq!(result.replacements, 0);
    }

    #[test]
    fn pem_private_key_is_redacted() {
        let redactor = Redactor::new();
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy0AHB7MhgHcTz6sE2I2yPB\n-----END RSA PRIVATE KEY-----";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:pem-block]"));
        assert!(!result.text.contains("MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn"));
        assert!(result.applied_rules.contains(&"pem-key".to_owned()));
    }

    #[test]
    fn aws_access_key_is_redacted() {
        let redactor = Redactor::new();
        let input = "AKIAIOSFODNN7EXAMPLE";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:aws-access-key]"));
        assert!(!result.text.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn aws_secret_key_is_redacted() {
        let redactor = Redactor::new();
        let input = "aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:aws-secret-key]"));
    }

    #[test]
    fn gcp_private_key_is_redacted() {
        let redactor = Redactor::new();
        let input = r#""private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg...""#;
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:gcp-private-key]"));
    }

    #[test]
    fn azure_connection_string_is_redacted() {
        let redactor = Redactor::new();
        let input = "DefaultEndpointsProtocol=https;AccountName=myacct;AccountKey=abcdefghijklmnopqrstuvwxyz0123456789+/==";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:azure-conn-str]"));
    }

    #[test]
    fn embedded_cred_url_is_redacted() {
        let redactor = Redactor::new();
        let input =
            "git clone https://user:ghp_abc123def456ghi789jkl012mno345pqr@github.com/repo.git";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:embedded-cred]@"));
        assert!(!result
            .text
            .contains("ghp_abc123def456ghi789jkl012mno345pqr"));
    }

    #[test]
    fn cookie_header_is_redacted() {
        let redactor = Redactor::new();
        let input = "Cookie: session=abc123def456ghi789; token=xyz789";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:cookie]"));
    }

    #[test]
    fn session_id_is_redacted() {
        let redactor = Redactor::new();
        let input = "session_id=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:session-id]"));
    }

    #[test]
    fn csrf_token_is_redacted() {
        let redactor = Redactor::new();
        let input = "csrf_token=abc123def456ghi789jkl012mno345pqr";
        let result = redactor.redact(input);
        assert!(result.text.contains("[REDACTED:csrf-token]"));
    }

    #[test]
    fn multiple_rules_apply_independently() {
        let redactor = Redactor::new();
        let input = concat!(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature\n",
            "DATABASE_PASSWORD=sup3rS3cretP@ssw0rd!\n",
            "git clone https://user:ghp_abc123def456ghi789jkl012mno345pqr@github.com/repo.git",
        );
        let result = redactor.redact(input);
        assert!(result.replacements >= 3);
        assert!(result.applied_rules.contains(&"api-key".to_owned()));
        assert!(result.applied_rules.contains(&"env-secret".to_owned()));
        assert!(result
            .applied_rules
            .contains(&"embedded-cred-url".to_owned()));
    }

    #[test]
    fn compiler_diagnostics_not_false_positives() {
        let redactor = Redactor::new();
        let input = concat!(
            "warning: unused variable: `token`\n",
            "error[E0596]: cannot borrow `secret_key` as mutable\n",
            "   --> src/main.rs:42:5\n",
            "    |\n",
            "42  |     let mut secret_key = String::new();\n",
            "    |         ^^^^^^^^^^^^^ help: ...",
        );
        let result = redactor.redact(input);
        assert_eq!(result.text, input);
        assert_eq!(result.replacements, 0);
    }

    #[test]
    fn short_api_key_value_not_redacted() {
        let redactor = Redactor::new();
        let input = "api_key=abc";
        let result = redactor.redact(input);
        assert_eq!(result.text, input);
        assert_eq!(result.replacements, 0);
    }

    #[test]
    fn redacted_output_metadata_is_correct() {
        let redactor = Redactor::new();
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let result = redactor.redact(input);
        assert_eq!(result.replacements, 1);
        assert_eq!(result.applied_rules.len(), 1);
        assert_eq!(result.applied_rules[0], "api-key");
    }

    #[test]
    fn redactor_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Redactor>();
    }
}
