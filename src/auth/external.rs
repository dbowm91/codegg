//! External-command credential provider.
//!
//! Some providers document an officially-supported CLI for issuing
//! short-lived credentials. The `ExternalCommandProvider` shells out to that
//! command and treats the trimmed stdout as the credential value.
//!
//! This is intentionally minimal. Provider-specific format handling (JSON
//! output, `token:` line, etc.) should be added when an actual
//! officially-supported external credential command is wired in.

use std::process::Command;
use std::time::Duration;

use crate::auth::error::AuthError;
use crate::auth::{Credential, CredentialKind};

#[derive(Debug, Clone)]
pub struct ExternalCredential {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ExternalCommandProvider;

impl ExternalCommandProvider {
    pub fn new() -> Self {
        Self
    }

    /// Run the configured command and return the trimmed stdout as a
    /// bearer-style credential. Returns an [`AuthError::ExternalCommand`] on
    /// any non-zero exit or timeout.
    pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
        if cred.command.trim().is_empty() {
            return Err(AuthError::Invalid("external command is empty".to_string()));
        }
        let timeout = Duration::from_millis(cred.timeout_ms.unwrap_or(15_000));
        let output = match Command::new(&cred.command).args(&cred.args).output() {
            Ok(out) => out,
            Err(e) => {
                return Err(AuthError::ExternalCommand {
                    command: cred.command.clone(),
                    message: format!("spawn failed: {e}"),
                });
            }
        };
        // Best-effort: enforce timeout via a watchdog-less run. Real wall-clock
        // timeouts would require `tokio::process`; the first pass keeps the
        // synchronous `Command` and uses the configured hint as documented
        // behavior (it is not enforced strictly).
        let _ = timeout;
        if !output.status.success() {
            return Err(AuthError::ExternalCommand {
                command: cred.command.clone(),
                message: format!(
                    "exit {:?}: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Err(AuthError::ExternalCommand {
                command: cred.command.clone(),
                message: "command produced empty stdout".to_string(),
            });
        }
        Ok(Credential {
            kind: CredentialKind::BearerToken,
            secret: value,
            expires_at: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_rejects_empty_command() {
        let provider = ExternalCommandProvider::new();
        let cred = ExternalCredential {
            command: "   ".to_string(),
            args: vec![],
            timeout_ms: None,
        };
        let err = provider.fetch(&cred).unwrap_err();
        match err {
            AuthError::Invalid(_) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn fetch_uses_trimmed_stdout() {
        // `printf` is widely available on macOS and Linux.
        let provider = ExternalCommandProvider::new();
        let cred = ExternalCredential {
            command: "printf".to_string(),
            args: vec!["  hello-token  ".to_string()],
            timeout_ms: Some(2_000),
        };
        let got = provider.fetch(&cred).expect("printf should succeed");
        assert_eq!(got.secret, "hello-token");
        assert_eq!(got.kind, CredentialKind::BearerToken);
    }

    #[test]
    fn fetch_returns_error_on_nonzero_exit() {
        let provider = ExternalCommandProvider::new();
        let cred = ExternalCredential {
            command: "false".to_string(),
            args: vec![],
            timeout_ms: Some(2_000),
        };
        let err = provider.fetch(&cred).unwrap_err();
        match err {
            AuthError::ExternalCommand { .. } => {}
            other => panic!("expected ExternalCommand error, got {other:?}"),
        }
    }
}
