//! Auth resolution.
//!
//! [`AuthResolver`] turns a provider id, an [`AuthConfig`], and the legacy
//! `ProviderConfig` into a concrete [`Credential`]. The priority order for
//! API-key-compatible providers is:
//!
//! 1. explicit `AuthConfig::ApiKey.env` env var, if set;
//! 2. conventional env var `{PROVIDER}_API_KEY` (backward compatible);
//! 3. explicit `AuthConfig::ApiKey.value`;
//! 4. `AuthConfig::ApiKey.encrypted_value` (decrypted with master key);
//! 5. user-level [`CredentialStore`] lookup;
//! 6. legacy `ProviderConfig::api_key` and
//!    `ProviderConfig::encrypted_api_key` (already decrypted by config
//!    loading when a master key is present).
//!
//! OAuth and external-command variants follow their own paths. OAuth is
//! recognized but returns [`AuthError::Unsupported`].

use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::credential::CredentialKind;
use crate::auth::error::AuthError;
use crate::auth::external::{ExternalCommandProvider, ExternalCredential};
use crate::auth::store::CredentialStore;
use crate::auth::{AuthConfig, Credential};

/// Inputs available to the resolver beyond the [`AuthConfig`] itself.
#[derive(Debug, Clone, Default)]
pub struct ResolverContext {
    /// Provider id, used for env var names and store lookups. Should be
    /// uppercased before being used as an env var prefix.
    pub provider_id: String,
    /// Optional account id, used for store lookups and account-scoped
    /// external commands.
    pub account_id: Option<String>,
    /// Legacy `ProviderConfig::api_key` (post-decryption).
    pub legacy_api_key: Option<String>,
    /// Legacy `ProviderConfig::encrypted_api_key` already decrypted (rare
    /// path; normally decryption happens in `decrypt_provider_keys`).
    pub legacy_decrypted: Option<String>,
    /// Optional pre-resolved user-store credential.
    pub store: Option<Arc<CredentialStore>>,
    /// Optional override env var name. Takes precedence over both the
    /// explicit `AuthConfig::ApiKey.env` and the conventional name. This
    /// exists primarily for tests.
    pub env_override: Option<String>,
}

/// Result of a successful resolution.
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub credential: Credential,
    /// Where the credential came from. Useful for diagnostics but never
    /// includes the secret itself.
    pub source: ResolvedAuthSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedAuthSource {
    EnvExplicit,
    EnvConventional,
    InlineValue,
    EncryptedConfig,
    UserStore,
    LegacyApiKey,
    LegacyDecrypted,
    ExternalCommand,
}

impl ResolvedAuthSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolvedAuthSource::EnvExplicit => "env(explicit)",
            ResolvedAuthSource::EnvConventional => "env(conventional)",
            ResolvedAuthSource::InlineValue => "config(inline)",
            ResolvedAuthSource::EncryptedConfig => "config(encrypted)",
            ResolvedAuthSource::UserStore => "user_store",
            ResolvedAuthSource::LegacyApiKey => "legacy(api_key)",
            ResolvedAuthSource::LegacyDecrypted => "legacy(decrypted)",
            ResolvedAuthSource::ExternalCommand => "external_command",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthResolver {
    external: ExternalCommandProvider,
}

impl AuthResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a credential for `auth` and `ctx`. Returns
    /// `Ok(None)` if the resolver found nothing configured. Returns
    /// `Err(AuthError::Unsupported)` for recognized-but-unimplemented
    /// auth modes (notably OAuth device-code).
    pub fn resolve(
        &self,
        auth: Option<&AuthConfig>,
        ctx: &ResolverContext,
    ) -> Result<Option<ResolvedAuth>, AuthError> {
        if let Some(cfg) = auth {
            match cfg {
                AuthConfig::ApiKey {
                    env,
                    value,
                    encrypted_value,
                } => {
                    if let Some(env_name) = ctx.env_override.as_deref().or(env.as_deref()) {
                        if let Some(v) = read_env(env_name) {
                            return Ok(Some(resolved(
                                Credential::api_key(v),
                                ResolvedAuthSource::EnvExplicit,
                            )));
                        }
                    }
                    let conventional = conventional_env_for(&ctx.provider_id);
                    if let Some(v) = read_env(&conventional) {
                        return Ok(Some(resolved(
                            Credential::api_key(v),
                            ResolvedAuthSource::EnvConventional,
                        )));
                    }
                    if let Some(v) = value {
                        if !v.is_empty() {
                            return Ok(Some(resolved(
                                Credential::api_key(v.clone()),
                                ResolvedAuthSource::InlineValue,
                            )));
                        }
                    }
                    if let Some(enc) = encrypted_value {
                        if let Some(master) = crate::config::encryption::get_master_key() {
                            match crate::crypto::decrypt_from_string(enc, &master) {
                                Ok(plain) => {
                                    return Ok(Some(resolved(
                                        Credential::api_key(plain),
                                        ResolvedAuthSource::EncryptedConfig,
                                    )));
                                }
                                Err(e) => {
                                    return Err(AuthError::Crypto(format!(
                                        "decrypt encrypted_value: {e}"
                                    )));
                                }
                            }
                        } else {
                            return Err(AuthError::MasterKeyMissing);
                        }
                    }
                }
                AuthConfig::Stored { account_id } => {
                    let store = ctx
                        .store
                        .as_ref()
                        .ok_or_else(|| AuthError::NotFound(ctx.provider_id.clone()))?;
                    let account = account_id.clone().or_else(|| ctx.account_id.clone());
                    if let Some(plain) =
                        store.get_plaintext(&ctx.provider_id, account.as_deref(), |s| {
                            s.kind == CredentialKind::ApiKey
                        })?
                    {
                        return Ok(Some(resolved(
                            Credential::api_key(plain),
                            ResolvedAuthSource::UserStore,
                        )));
                    }
                    return Err(AuthError::NotFound(ctx.provider_id.clone()));
                }
                AuthConfig::ExternalCommand {
                    command,
                    args,
                    timeout_ms,
                } => {
                    let cred = ExternalCredential {
                        command: command.clone(),
                        args: args.clone(),
                        timeout_ms: *timeout_ms,
                    };
                    let got = self.external.fetch(&cred)?;
                    return Ok(Some(resolved(got, ResolvedAuthSource::ExternalCommand)));
                }
                AuthConfig::OAuthDevice { .. } => {
                    return Err(AuthError::Unsupported("OAuthDevice".to_string()));
                }
                AuthConfig::None => return Ok(None),
            }
        }

        // No auth: try conventional env var, then legacy fields.
        if let Some(env_name) = ctx.env_override.as_deref() {
            if let Some(v) = read_env(env_name) {
                return Ok(Some(resolved(
                    Credential::api_key(v),
                    ResolvedAuthSource::EnvExplicit,
                )));
            }
        }
        let conventional = conventional_env_for(&ctx.provider_id);
        if let Some(v) = read_env(&conventional) {
            return Ok(Some(resolved(
                Credential::api_key(v),
                ResolvedAuthSource::EnvConventional,
            )));
        }
        if let Some(ref k) = ctx.legacy_api_key {
            if !k.is_empty() {
                return Ok(Some(resolved(
                    Credential::api_key(k.clone()),
                    ResolvedAuthSource::LegacyApiKey,
                )));
            }
        }
        if let Some(ref k) = ctx.legacy_decrypted {
            if !k.is_empty() {
                return Ok(Some(resolved(
                    Credential::api_key(k.clone()),
                    ResolvedAuthSource::LegacyDecrypted,
                )));
            }
        }
        if let Some(store) = ctx.store.as_ref() {
            if let Some(plain) =
                store.get_plaintext(&ctx.provider_id, ctx.account_id.as_deref(), |s| {
                    s.kind == CredentialKind::ApiKey
                })?
            {
                return Ok(Some(resolved(
                    Credential::api_key(plain),
                    ResolvedAuthSource::UserStore,
                )));
            }
        }
        Ok(None)
    }
}

fn resolved(credential: Credential, source: ResolvedAuthSource) -> ResolvedAuth {
    ResolvedAuth { credential, source }
}

fn read_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn conventional_env_for(provider_id: &str) -> String {
    let upper = provider_id.to_uppercase().replace('-', "_");
    format!("{upper}_API_KEY")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_var() -> &'static str {
        "CODEGG_TEST_AUTH_VAR_DO_NOT_USE"
    }

    #[test]
    fn explicit_env_wins() {
        let _guard = crate::auth::test_support::lock_env();

        let prev = std::env::var(unique_var()).ok();
        let prev_openai = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::set_var(unique_var(), "explicit-key");
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "explicit_env_wins_provider".to_string(),
            env_override: Some(unique_var().to_string()),
            ..Default::default()
        };
        let cfg = AuthConfig::ApiKey {
            env: Some(unique_var().to_string()),
            value: None,
            encrypted_value: None,
        };
        let r = resolver
            .resolve(Some(&cfg), &ctx)
            .expect("ok")
            .expect("some");
        assert_eq!(r.source, ResolvedAuthSource::EnvExplicit);
        assert_eq!(r.credential.secret, "explicit-key");
        if let Some(v) = prev {
            std::env::set_var(unique_var(), v);
        } else {
            std::env::remove_var(unique_var());
        }
        if let Some(v) = prev_openai {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }

    #[test]
    fn falls_back_to_conventional_env() {
        let _guard = crate::auth::test_support::lock_env();

        let prev = std::env::var("OPENAI_API_KEY").ok();
        std::env::set_var("OPENAI_API_KEY", "conv-key");
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "openai".to_string(),
            ..Default::default()
        };
        let cfg = AuthConfig::ApiKey {
            env: None,
            value: None,
            encrypted_value: None,
        };
        let r = resolver
            .resolve(Some(&cfg), &ctx)
            .expect("ok")
            .expect("some");
        assert_eq!(r.source, ResolvedAuthSource::EnvConventional);
        assert_eq!(r.credential.secret, "conv-key");
        if let Some(v) = prev {
            std::env::set_var("OPENAI_API_KEY", v);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    fn inline_value_used_when_no_env() {
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "xai".to_string(),
            ..Default::default()
        };
        let cfg = AuthConfig::ApiKey {
            env: None,
            value: Some("inline-key".to_string()),
            encrypted_value: None,
        };
        let r = resolver
            .resolve(Some(&cfg), &ctx)
            .expect("ok")
            .expect("some");
        assert_eq!(r.source, ResolvedAuthSource::InlineValue);
        assert_eq!(r.credential.secret, "inline-key");
    }

    #[test]
    fn falls_back_to_legacy_api_key() {
        let _guard = crate::auth::test_support::lock_env();

        // Use a unique provider id so the conventional env probe is
        // guaranteed empty even when other tests have set real env vars.
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "code_legacy_only_provider".to_string(),
            legacy_api_key: Some("legacy-key".to_string()),
            ..Default::default()
        };
        let r = resolver.resolve(None, &ctx).expect("ok").expect("some");
        assert_eq!(r.source, ResolvedAuthSource::LegacyApiKey);
        assert_eq!(r.credential.secret, "legacy-key");
    }

    #[test]
    fn resolves_none_when_nothing_configured() {
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "absent_provider".to_string(),
            ..Default::default()
        };
        let r = resolver.resolve(None, &ctx).expect("ok");
        assert!(r.is_none());
    }

    #[test]
    fn oauth_device_returns_unsupported() {
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "openai".to_string(),
            ..Default::default()
        };
        let cfg = AuthConfig::OAuthDevice {
            client_id: "abc".to_string(),
            scopes: vec!["read".to_string()],
            auth_url: "https://example/auth".to_string(),
            token_url: "https://example/token".to_string(),
        };
        let err = resolver.resolve(Some(&cfg), &ctx).unwrap_err();
        assert!(matches!(err, AuthError::Unsupported(_)));
    }

    #[test]
    fn auth_none_returns_none() {
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "openai".to_string(),
            legacy_api_key: Some("should-not-be-used".to_string()),
            ..Default::default()
        };
        let r = resolver.resolve(Some(&AuthConfig::None), &ctx).expect("ok");
        assert!(r.is_none());
    }

    #[test]
    fn encrypted_value_requires_master_key() {
        let _guard = crate::auth::test_support::lock_env();

        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
        let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
        let prev_openai = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("CODEGG_MASTER_KEY");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "encrypted_only".to_string(),
            ..Default::default()
        };
        let cfg = AuthConfig::ApiKey {
            env: None,
            value: None,
            encrypted_value: Some("v2:00".to_string()),
        };
        let err = resolver.resolve(Some(&cfg), &ctx).unwrap_err();
        assert!(matches!(err, AuthError::MasterKeyMissing));
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
        if let Some(v) = prev_enc {
            std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
        }
        if let Some(v) = prev_opencode {
            std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
        }
        if let Some(v) = prev_openai {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }
}

/// Helper for tests and one-off diagnostics: summarize the providers known
/// to this build and the env-var names the resolver will probe.
pub fn conventional_env_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("anthropic", "ANTHROPIC_API_KEY");
    m.insert("openai", "OPENAI_API_KEY");
    m.insert("google", "GOOGLE_API_KEY");
    m.insert("openrouter", "OPENROUTER_API_KEY");
    m.insert("opencode_zen", "OPENCODE_ZEN_API_KEY");
    m.insert("mistral", "MISTRAL_API_KEY");
    m.insert("groq", "GROQ_API_KEY");
    m.insert("deepinfra", "DEEPINFRA_API_KEY");
    m.insert("cerebras", "CEREBRAS_API_KEY");
    m.insert("cohere", "COHERE_API_KEY");
    m.insert("together", "TOGETHERAI_API_KEY");
    m.insert("perplexity", "PERPLEXITY_API_KEY");
    m.insert("xai", "XAI_API_KEY");
    m.insert("venice", "VENICE_API_KEY");
    m.insert("minimax", "MINIMAX_API_KEY");
    m.insert("opencode_go", "OPENCODE_GO_API_KEY");
    m.insert("generalcompute", "GENERALCOMPUTE_API_KEY");
    m
}
