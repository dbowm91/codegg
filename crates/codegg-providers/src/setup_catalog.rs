//! Pre-credential provider definitions for setup and durable provisioning.
//!
//! [`ProviderRegistry`] contains executable providers only after credentials
//! resolve, so it cannot drive `/connect`: an unconfigured provider has no
//! instance to list. This module is the shared metadata + construction policy
//! layer consumed by startup registration, the durable
//! [`crate::connection::ProviderConnectionFactory`], and the daemon's generic
//! provider-connection provisioner.
//!
//! A definition carries only non-secret executable metadata: stable ID,
//! display name, whether CodeGG can onboard it with the currently implemented
//! local form, accepted credential capability, endpoint policy, construction
//! strategy, and model validation/discovery strategy. It never holds secrets,
//! credential values, or authorization policy.
//!
//! Providers requiring authentication that is not representable by the current
//! safe form (cloud signing, OAuth/device flows, multi-field account
//! credentials) must be registered here with `connectable = false` until
//! typed onboarding fields and a durable builder exist. Do not fake support
//! by coercing such auth into one API-key box.

use crate::auth_types::{Credential, CredentialCapability, CredentialKind};
use crate::connection::ConnectionError;
use crate::Provider;

// ── Fixed endpoint constants ─────────────────────────────────────────────
// These are the single source of truth for built-in base URLs. The provider
// constructors in `additional` (and the native defaults below) reference
// these constants so the setup catalog and runtime construction cannot drift.

pub const MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";
pub const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const DEEPINFRA_BASE_URL: &str = "https://api.deepinfra.com/v1/openai";
pub const CEREBRAS_BASE_URL: &str = "https://api.cerebras.ai/v1";
pub const COHERE_BASE_URL: &str = "https://api.cohere.ai/compatibility/v1";
pub const TOGETHER_BASE_URL: &str = "https://api.together.xyz/v1";
pub const PERPLEXITY_BASE_URL: &str = "https://api.perplexity.ai";
pub const XAI_BASE_URL: &str = "https://api.x.ai/v1";
pub const VENICE_BASE_URL: &str = "https://api.venice.ai/api/v1";
pub const GENERALCOMPUTE_BASE_URL: &str = "https://api.generalcompute.com/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const GOOGLE_ENDPOINT: &str = "https://generativelanguage.googleapis.com";
pub const OPENROUTER_ENDPOINT: &str = "https://openrouter.ai/api/v1";
pub const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";
pub const MINIMAX_BASE_URL: &str = "https://api.minimax.io/anthropic";
/// Durable-connection mirror of the OpenCode Go base URL.
///
/// The **authoritative** value is the shared EggPool provider-profile contract
/// (`crate::provider_profile::shared_base_url("opencode_go")`), which is what
/// the runtime provider actually resolves against. This constant exists only
/// because `SetupEndpointPolicy::Fixed` stores a `&'static str` for the durable
/// connection record, and it must never drift from the shared profile.
/// `opencode_go_base_url_mirrors_the_shared_profile` pins the two together.
pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";

/// Default port for the Eggpool local/shared proxy preset.
pub const EGGPOOL_PRESET_DEFAULT_PORT: u16 = crate::EGGPOOL_DEFAULT_PORT;

/// Stable ID for the generic OpenAI-compatible custom upstream entry.
pub const CUSTOM_COMPATIBLE_ID: &str = "custom";
/// Stable ID for the Eggpool proxy preset.
pub const EGGPOOL_PRESET_ID: &str = "eggpool";
/// Stable ID for the Azure OpenAI entry (endpoint-required, native transport).
pub const AZURE_ID: &str = "azure";

/// How a provider's endpoint is supplied during onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupEndpointPolicy {
    /// The endpoint is fixed by the definition; callers must not supply one.
    Fixed { base_url: &'static str },
    /// The provider works without an endpoint; a caller may override it.
    /// `None` means the provider implementation owns its internal default.
    OptionalOverride {
        default_base_url: Option<&'static str>,
    },
    /// The caller must supply a full base URL (custom/compatible upstreams,
    /// Azure deployments).
    RequiredEndpoint,
    /// Eggpool-style host + optional port + TLS policy preset.
    ProxyPreset { default_port: u16 },
}

/// How a provisioned connection discovers its model catalog.
///
/// This type describes **catalog discovery only**. It is not, and must never
/// become, a credential-verification contract: whether a successful probe is
/// allowed to prove the credential is stated separately by
/// [`Self::credential_evidence`], and the two live in different types so the
/// distinction cannot be lost. See [`crate::qualification`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProbeStrategy {
    /// Construct the provider through the canonical builder and call
    /// `Provider::models()` behind the operation cancellation/timeout
    /// boundary, normalizing the result into the bounded connection catalog.
    ///
    /// This is a **catalog** strategy. `Provider::models()` is not a
    /// credential contract: several providers return a local/static array with
    /// no network I/O, generic OpenAI-compatible discovery is deliberately
    /// best-effort and falls back rather than proving auth, and some provider
    /// `/models` endpoints are publicly readable. A successful probe here
    /// therefore always leaves the credential
    /// [`crate::qualification::CredentialVerification::Unverified`].
    ProviderCatalog,
    /// Use the strict OpenAI-compatible `/models` probe (redirect, body,
    /// and model-count bounds) shared with the Eggpool preset.
    ///
    /// Unlike [`Self::ProviderCatalog`] this is a genuinely authenticated,
    /// non-billable metadata request: the upstream evaluates the presented
    /// credential and a 2xx response is proof it was accepted.
    AuthenticatedCompatibleCatalog,
}

impl SetupProbeStrategy {
    /// What a successful probe of this strategy is permitted to prove about the
    /// credential. [`Self::ProviderCatalog`] deliberately yields
    /// [`CredentialEvidence::CatalogOnly`].
    pub const fn credential_evidence(self) -> crate::qualification::CredentialEvidence {
        use crate::qualification::CredentialEvidence;
        match self {
            // `Provider::models()` success is catalog evidence only.
            Self::ProviderCatalog => CredentialEvidence::CatalogOnly,
            // The strict probe authenticates the credential upstream.
            Self::AuthenticatedCompatibleCatalog => CredentialEvidence::Authenticated,
        }
    }
}

/// Which provider implementation a durable connection must be built with.
///
/// Specialized implementations (custom headers, request shapes, session
/// affinity) keep their own constructor here rather than being coerced to
/// generic OpenAI-compatible transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupConstruction {
    OpenAiNative,
    AnthropicNative,
    GoogleNative,
    AzureNative,
    OpenRouterNative,
    ZenNative,
    MinimaxAnthropic,
    OpenAiCompatibleFixed,
    XaiCustom,
    OpenCodeGoAffinity,
    CompatibleProxy,
    CustomCompatible,
}

/// One pre-credential provider definition. Secret-free by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDefinition {
    /// Stable provider ID used by storage, protocol, and the catalog.
    pub id: &'static str,
    /// Human-readable name for selection surfaces.
    pub display_name: &'static str,
    /// Whether CodeGG can onboard this provider with the currently
    /// implemented local form. `false` entries are known but not selectable
    /// until typed onboarding fields and a durable builder exist.
    pub connectable: bool,
    /// Accepted credential capability/kind.
    pub credential_capability: CredentialCapability,
    /// How the endpoint is supplied.
    pub endpoint_policy: SetupEndpointPolicy,
    /// Which implementation the durable builder must construct.
    pub construction: SetupConstruction,
    /// How provisioning validates the connection and discovers models.
    pub probe_strategy: SetupProbeStrategy,
    /// Conventional environment variable hint for presentation only.
    /// Never an authorization Grant.
    pub env_var: Option<&'static str>,
    /// One-line non-secret description for selection surfaces.
    pub description: &'static str,
}

impl ProviderDefinition {
    /// Fixed endpoint when the policy pins one, if any.
    pub const fn fixed_base_url(self) -> Option<&'static str> {
        match self.endpoint_policy {
            SetupEndpointPolicy::Fixed { base_url } => Some(base_url),
            SetupEndpointPolicy::OptionalOverride { default_base_url } => default_base_url,
            SetupEndpointPolicy::RequiredEndpoint | SetupEndpointPolicy::ProxyPreset { .. } => None,
        }
    }

    /// Whether callers may supply an endpoint for this definition.
    pub const fn accepts_endpoint(self) -> bool {
        match self.endpoint_policy {
            SetupEndpointPolicy::Fixed { .. } => false,
            SetupEndpointPolicy::OptionalOverride { .. }
            | SetupEndpointPolicy::RequiredEndpoint
            | SetupEndpointPolicy::ProxyPreset { .. } => true,
        }
    }

    /// Whether callers must supply an endpoint for this definition.
    pub const fn requires_endpoint(self) -> bool {
        match self.endpoint_policy {
            SetupEndpointPolicy::RequiredEndpoint | SetupEndpointPolicy::ProxyPreset { .. } => true,
            SetupEndpointPolicy::Fixed { .. } | SetupEndpointPolicy::OptionalOverride { .. } => {
                false
            }
        }
    }
}

const fn definition(
    id: &'static str,
    display_name: &'static str,
    credential_capability: CredentialCapability,
    endpoint_policy: SetupEndpointPolicy,
    construction: SetupConstruction,
    probe_strategy: SetupProbeStrategy,
    env_var: Option<&'static str>,
    description: &'static str,
) -> ProviderDefinition {
    ProviderDefinition {
        id,
        display_name,
        connectable: true,
        credential_capability,
        endpoint_policy,
        construction,
        probe_strategy,
        env_var,
        description,
    }
}

/// The canonical pre-credential provider catalog.
///
/// Every ID in [`crate::builtin_registration_order`] must appear here with an
/// explicit setup disposition; the coverage test pins that invariant so a new
/// built-in cannot land without one.
pub fn provider_setup_catalog() -> &'static [ProviderDefinition] {
    use CredentialCapability::{ApiKeyOnly, ApiKeyOrBearer};
    static CATALOG: &[ProviderDefinition] = &[
        definition(
            "anthropic",
            "Anthropic",
            ApiKeyOnly,
            SetupEndpointPolicy::OptionalOverride {
                default_base_url: Some(ANTHROPIC_BASE_URL),
            },
            SetupConstruction::AnthropicNative,
            SetupProbeStrategy::ProviderCatalog,
            Some("ANTHROPIC_API_KEY"),
            "Anthropic Claude API",
        ),
        definition(
            "openai",
            "OpenAI",
            ApiKeyOnly,
            SetupEndpointPolicy::OptionalOverride {
                default_base_url: Some(OPENAI_BASE_URL),
            },
            SetupConstruction::OpenAiNative,
            SetupProbeStrategy::ProviderCatalog,
            Some("OPENAI_API_KEY"),
            "OpenAI API",
        ),
        definition(
            "google",
            "Google",
            ApiKeyOnly,
            SetupEndpointPolicy::Fixed {
                base_url: GOOGLE_ENDPOINT,
            },
            SetupConstruction::GoogleNative,
            SetupProbeStrategy::ProviderCatalog,
            Some("GOOGLE_API_KEY"),
            "Google Gemini API",
        ),
        definition(
            "openrouter",
            "OpenRouter",
            ApiKeyOnly,
            SetupEndpointPolicy::Fixed {
                base_url: OPENROUTER_ENDPOINT,
            },
            SetupConstruction::OpenRouterNative,
            SetupProbeStrategy::ProviderCatalog,
            Some("OPENROUTER_API_KEY"),
            "OpenRouter unified gateway",
        ),
        definition(
            "opencode_zen",
            "OpenCode Zen",
            ApiKeyOnly,
            SetupEndpointPolicy::Fixed {
                base_url: OPENCODE_ZEN_BASE_URL,
            },
            SetupConstruction::ZenNative,
            SetupProbeStrategy::ProviderCatalog,
            Some("OPENCODE_ZEN_API_KEY"),
            "OpenCode Zen gateway",
        ),
        definition(
            "mistral",
            "Mistral",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: MISTRAL_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("MISTRAL_API_KEY"),
            "Mistral API (OpenAI-compatible)",
        ),
        definition(
            "groq",
            "Groq",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: GROQ_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("GROQ_API_KEY"),
            "Groq API (OpenAI-compatible)",
        ),
        definition(
            "deepinfra",
            "DeepInfra",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: DEEPINFRA_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("DEEPINFRA_API_KEY"),
            "DeepInfra API (OpenAI-compatible)",
        ),
        definition(
            "cerebras",
            "Cerebras",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: CEREBRAS_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("CEREBRAS_API_KEY"),
            "Cerebras API (OpenAI-compatible)",
        ),
        definition(
            "cohere",
            "Cohere",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: COHERE_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("COHERE_API_KEY"),
            "Cohere API (OpenAI-compatible)",
        ),
        definition(
            "together",
            "Together AI",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: TOGETHER_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("TOGETHERAI_API_KEY"),
            "Together AI API (OpenAI-compatible)",
        ),
        definition(
            "perplexity",
            "Perplexity",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: PERPLEXITY_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("PERPLEXITY_API_KEY"),
            "Perplexity API (OpenAI-compatible)",
        ),
        definition(
            "xai",
            "xAI",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: XAI_BASE_URL,
            },
            SetupConstruction::XaiCustom,
            SetupProbeStrategy::ProviderCatalog,
            Some("XAI_API_KEY"),
            "xAI API (OpenAI-compatible)",
        ),
        definition(
            "venice",
            "Venice",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: VENICE_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("VENICE_API_KEY"),
            "Venice API (OpenAI-compatible)",
        ),
        definition(
            "minimax",
            "MiniMax",
            ApiKeyOnly,
            SetupEndpointPolicy::Fixed {
                base_url: MINIMAX_BASE_URL,
            },
            SetupConstruction::MinimaxAnthropic,
            SetupProbeStrategy::ProviderCatalog,
            Some("MINIMAX_API_KEY"),
            "MiniMax API (Anthropic-compatible)",
        ),
        definition(
            "opencode_go",
            "OpenCode Go",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: OPENCODE_GO_BASE_URL,
            },
            SetupConstruction::OpenCodeGoAffinity,
            SetupProbeStrategy::ProviderCatalog,
            Some("OPENCODE_GO_API_KEY"),
            "OpenCode Go gateway (OpenAI-compatible)",
        ),
        definition(
            "generalcompute",
            "GeneralCompute",
            ApiKeyOrBearer,
            SetupEndpointPolicy::Fixed {
                base_url: GENERALCOMPUTE_BASE_URL,
            },
            SetupConstruction::OpenAiCompatibleFixed,
            SetupProbeStrategy::ProviderCatalog,
            Some("GENERALCOMPUTE_API_KEY"),
            "GeneralCompute API (OpenAI-compatible)",
        ),
        definition(
            EGGPOOL_PRESET_ID,
            "Eggpool",
            ApiKeyOrBearer,
            SetupEndpointPolicy::ProxyPreset {
                default_port: EGGPOOL_PRESET_DEFAULT_PORT,
            },
            SetupConstruction::CompatibleProxy,
            SetupProbeStrategy::AuthenticatedCompatibleCatalog,
            None,
            "Eggpool local/shared proxy (OpenAI-compatible)",
        ),
        definition(
            CUSTOM_COMPATIBLE_ID,
            "Custom OpenAI-compatible",
            ApiKeyOrBearer,
            SetupEndpointPolicy::RequiredEndpoint,
            SetupConstruction::CustomCompatible,
            SetupProbeStrategy::AuthenticatedCompatibleCatalog,
            None,
            "Any OpenAI-compatible upstream with an endpoint",
        ),
        definition(
            AZURE_ID,
            "Azure OpenAI",
            ApiKeyOnly,
            SetupEndpointPolicy::RequiredEndpoint,
            SetupConstruction::AzureNative,
            SetupProbeStrategy::ProviderCatalog,
            None,
            "Azure OpenAI deployment endpoint",
        ),
    ];
    CATALOG
}

/// Look up one pre-credential definition by stable provider ID.
pub fn setup_definition(provider_id: &str) -> Option<&'static ProviderDefinition> {
    provider_setup_catalog()
        .iter()
        .find(|definition| definition.id == provider_id)
}

/// Fixed base URL for a definition, when the endpoint policy pins one.
pub fn fixed_base_url(provider_id: &str) -> Option<&'static str> {
    setup_definition(provider_id).and_then(|definition| definition.fixed_base_url())
}

fn display_or_default(display_name: &str, fallback: &str) -> String {
    if display_name.trim().is_empty() {
        fallback.to_string()
    } else {
        display_name.to_string()
    }
}

fn require_api_key(provider_id: &str, credential: &Credential) -> Result<(), ConnectionError> {
    if credential.kind != CredentialKind::ApiKey {
        return Err(ConnectionError::UnsupportedCredentialKind {
            provider_id: provider_id.to_string(),
            kind: credential.kind,
        });
    }
    Ok(())
}

/// Canonical durable provider builder shared by the connection factory and
/// the generic provisioner's direct-model validation.
///
/// `base_url` carries the stored/validated endpoint when the caller has one:
/// it is required for endpoint-requiring definitions, accepted as an override
/// for [`SetupEndpointPolicy::OptionalOverride`], and ignored for
/// [`SetupEndpointPolicy::Fixed`] definitions (the catalog URL wins so a
/// hand-built descriptor cannot silently repoint a fixed upstream).
///
/// Unknown provider IDs fall back to generic OpenAI-compatible transport when
/// a base URL is present, preserving the historical compatibility behavior
/// for stored `openai_compatible` rows and ad-hoc gateways. Unknown IDs
/// without a base URL are rejected.
pub fn build_durable_provider(
    provider_id: &str,
    credential: Credential,
    base_url: Option<&str>,
    display_name: &str,
) -> Result<Box<dyn Provider>, ConnectionError> {
    if let Some(definition) = setup_definition(provider_id) {
        if !definition.credential_capability.accepts(credential.kind) {
            return Err(ConnectionError::UnsupportedCredentialKind {
                provider_id: provider_id.to_string(),
                kind: credential.kind,
            });
        }
        let name = display_or_default(display_name, definition.display_name);
        match definition.construction {
            SetupConstruction::OpenAiNative => {
                require_api_key(provider_id, &credential)?;
                let mut config = crate::openai::OpenAiConfig::default_with_key(credential.secret);
                if let Some(base_url) = base_url {
                    config.base_url = base_url.to_string();
                }
                config.provider_id = provider_id.to_string();
                config.provider_name = name;
                Ok(Box::new(crate::openai::OpenAiProvider::new(config)))
            }
            SetupConstruction::AnthropicNative => {
                require_api_key(provider_id, &credential)?;
                let mut provider = crate::anthropic::AnthropicProvider::new(credential.secret)
                    .with_id(provider_id.to_string())
                    .with_name(name);
                if let Some(base_url) = base_url {
                    provider = provider.with_base_url(base_url.to_string());
                }
                Ok(Box::new(provider))
            }
            SetupConstruction::GoogleNative => {
                require_api_key(provider_id, &credential)?;
                Ok(Box::new(crate::google::GoogleProvider::new(
                    credential.secret,
                )))
            }
            SetupConstruction::AzureNative => {
                require_api_key(provider_id, &credential)?;
                let endpoint = base_url
                    .filter(|url| !url.trim().is_empty())
                    .ok_or_else(|| {
                        ConnectionError::InvalidDescriptor(
                            "azure connections require base_url".to_string(),
                        )
                    })?;
                Ok(Box::new(crate::azure::AzureProvider::new(
                    credential.secret,
                    endpoint.to_string(),
                )))
            }
            SetupConstruction::OpenRouterNative => {
                require_api_key(provider_id, &credential)?;
                Ok(Box::new(crate::openrouter::OpenRouterProvider::new(
                    credential.secret,
                )))
            }
            SetupConstruction::ZenNative => {
                require_api_key(provider_id, &credential)?;
                let mut provider = crate::opencode_zen::OpencodeZenProvider::new(credential.secret);
                if let Some(base_url) = base_url.filter(|url| !url.trim().is_empty()) {
                    provider = provider.with_base_url(base_url.to_string());
                }
                Ok(Box::new(provider))
            }
            SetupConstruction::MinimaxAnthropic => {
                require_api_key(provider_id, &credential)?;
                Ok(Box::new(crate::additional::create_minimax(
                    credential.secret,
                )))
            }
            SetupConstruction::OpenAiCompatibleFixed => {
                let base_url = definition.fixed_base_url().ok_or_else(|| {
                    ConnectionError::InvalidDescriptor(
                        "fixed-endpoint definition is missing its base URL".to_string(),
                    )
                })?;
                Ok(Box::new(
                    crate::openai_compatible::OpenAiCompatibleProvider::simple_with_credential(
                        provider_id,
                        &name,
                        credential,
                        base_url,
                    ),
                ))
            }
            SetupConstruction::XaiCustom => Ok(Box::new(crate::additional::create_xai(credential))),
            SetupConstruction::OpenCodeGoAffinity => {
                Ok(Box::new(crate::additional::create_opencode_go(credential)))
            }
            SetupConstruction::CompatibleProxy | SetupConstruction::CustomCompatible => {
                let base_url = base_url
                    .filter(|url| !url.trim().is_empty())
                    .ok_or_else(|| {
                        ConnectionError::InvalidDescriptor(format!(
                            "{provider_id} connections require base_url"
                        ))
                    })?;
                Ok(Box::new(
                    crate::openai_compatible::OpenAiCompatibleProvider::simple_with_credential(
                        provider_id,
                        &name,
                        credential,
                        base_url,
                    ),
                ))
            }
        }
    } else {
        // Compatibility fallback for stored rows that predate the catalog
        // (`openai_compatible`) and ad-hoc gateways: generic compatible
        // transport preserves the credential kind and stored endpoint.
        let base_url = base_url
            .filter(|url| !url.trim().is_empty())
            .ok_or_else(|| {
                ConnectionError::InvalidDescriptor(format!("unknown provider '{provider_id}'"))
            })?;
        let name = display_or_default(display_name, provider_id);
        Ok(Box::new(
            crate::openai_compatible::OpenAiCompatibleProvider::simple_with_credential(
                provider_id,
                &name,
                credential,
                base_url,
            ),
        ))
    }
}

/// C001 test-support: construct the OpenCode Go durable provider with only the
/// **capture origin** redirected, so a loopback capture server can observe the
/// exact request the production origin would receive.
///
/// Credential-capability policy is identical to the `OpenCodeGoAffinity` arm of
/// [`build_durable_provider`]; nothing else about the provider changes. The
/// shared profile still owns the path, the per-surface auth shape, and the
/// surface decision, so a captured path is the production path.
///
/// Compiled only under `cfg(test)` or the opt-in `capture-test-support`
/// feature. It is not reachable from configuration, the protocol, an environment
/// variable, or any CLI path.
#[cfg(any(test, feature = "capture-test-support"))]
pub fn build_opencode_go_with_capture_base(
    provider_id: &str,
    credential: Credential,
    capture_base: &str,
) -> Result<Box<dyn Provider>, ConnectionError> {
    if let Some(definition) = setup_definition(provider_id) {
        if !definition.credential_capability.accepts(credential.kind) {
            return Err(ConnectionError::UnsupportedCredentialKind {
                provider_id: provider_id.to_string(),
                kind: credential.kind,
            });
        }
    }
    Ok(Box::new(
        crate::opencode_go::OpenCodeGoProvider::new(credential).with_capture_base(capture_base),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_types::Credential;

    fn api_key(secret: &str) -> Credential {
        Credential::api_key(secret)
    }

    fn bearer(secret: &str) -> Credential {
        Credential::bearer(secret, None)
    }

    #[test]
    fn catalog_covers_builtin_registration_order_with_explicit_disposition() {
        for provider_id in crate::builtin_registration_order() {
            let definition = setup_definition(provider_id)
                .unwrap_or_else(|| panic!("builtin '{provider_id}' has no setup definition"));
            assert!(
                definition.connectable,
                "builtin '{provider_id}' must be explicitly connectable or carry a reason"
            );
            assert_eq!(
                definition.credential_capability,
                crate::credential_capability_for(provider_id),
                "catalog capability drifted from the registration matrix for '{provider_id}'"
            );
        }
    }

    #[test]
    fn catalog_capability_matrix_is_pinned() {
        use CredentialCapability::{ApiKeyOnly, ApiKeyOrBearer};
        let expected: &[(&str, CredentialCapability)] = &[
            ("anthropic", ApiKeyOnly),
            ("openai", ApiKeyOnly),
            ("google", ApiKeyOnly),
            ("openrouter", ApiKeyOnly),
            ("opencode_zen", ApiKeyOnly),
            ("mistral", ApiKeyOrBearer),
            ("groq", ApiKeyOrBearer),
            ("deepinfra", ApiKeyOrBearer),
            ("cerebras", ApiKeyOrBearer),
            ("cohere", ApiKeyOrBearer),
            ("together", ApiKeyOrBearer),
            ("perplexity", ApiKeyOrBearer),
            ("xai", ApiKeyOrBearer),
            ("venice", ApiKeyOrBearer),
            ("minimax", ApiKeyOnly),
            ("opencode_go", ApiKeyOrBearer),
            ("generalcompute", ApiKeyOrBearer),
            ("eggpool", ApiKeyOrBearer),
            ("custom", ApiKeyOrBearer),
            ("azure", ApiKeyOnly),
        ];
        assert_eq!(
            provider_setup_catalog().len(),
            expected.len(),
            "catalog size changed; update the pinned matrix deliberately"
        );
        for (provider_id, capability) in expected {
            let definition = setup_definition(provider_id)
                .unwrap_or_else(|| panic!("catalog is missing '{provider_id}'"));
            assert_eq!(
                definition.credential_capability, *capability,
                "capability changed for '{provider_id}'"
            );
        }
    }

    #[test]
    fn catalog_discovery_never_implies_credential_verification() {
        // Structural guard for the M010 invariant: a successful catalog probe
        // may only produce `Verified` when the strategy is a genuinely
        // authenticated metadata request. `Provider::models()` success is
        // catalog evidence only — several built-ins return local/static model
        // arrays with no network I/O, generic compatible discovery falls back
        // instead of proving auth, and some provider `/models` endpoints are
        // publicly readable.
        use crate::qualification::{CatalogOutcome, CredentialEvidence, CredentialVerification};
        let catalog_available = CatalogOutcome::available(4, "digest", 1);
        for definition in provider_setup_catalog() {
            let strategy = definition.probe_strategy;
            let evidence = strategy.credential_evidence();
            let classified =
                CredentialVerification::from_probe(evidence, &catalog_available, false);
            match strategy {
                SetupProbeStrategy::ProviderCatalog => {
                    assert_eq!(
                        evidence,
                        CredentialEvidence::CatalogOnly,
                        "'{}' must not claim authenticated evidence from Provider::models()",
                        definition.id
                    );
                    assert_eq!(
                        classified,
                        CredentialVerification::Unverified,
                        "'{}' reported a credential as verified from catalog discovery alone",
                        definition.id
                    );
                }
                SetupProbeStrategy::AuthenticatedCompatibleCatalog => {
                    assert_eq!(
                        classified,
                        CredentialVerification::Verified,
                        "'{}' uses the strict authenticated probe and must verify",
                        definition.id
                    );
                }
            }
        }
    }

    #[test]
    fn no_provider_uses_model_enumeration_as_an_auth_contract() {
        // Every built-in that has a non-empty static/local `models()` array
        // must be on the catalog-only strategy. This is the durable form of
        // "a static model list can never validate a key".
        for definition in provider_setup_catalog() {
            if matches!(
                definition.probe_strategy,
                SetupProbeStrategy::ProviderCatalog
            ) {
                continue;
            }
            assert!(
                !matches!(definition.construction, SetupConstruction::OpenAiNative),
                "'{}' reaches an authenticated probe through a construction \
                 that returns static models",
                definition.id
            );
        }
    }

    #[test]
    fn every_connectable_definition_builds_through_the_durable_builder() {
        for definition in provider_setup_catalog() {
            assert!(
                definition.connectable,
                "'{}' must stay connectable or this test must cover its reason",
                definition.id
            );
            let needs_endpoint = matches!(
                definition.endpoint_policy,
                SetupEndpointPolicy::RequiredEndpoint | SetupEndpointPolicy::ProxyPreset { .. }
            );
            let base_url = needs_endpoint.then_some("https://provision.example/v1");
            // API keys must always work for connectable definitions.
            let provider = build_durable_provider(
                definition.id,
                api_key("catalog-build-key"),
                base_url,
                "Catalog",
            )
            .unwrap_or_else(|error| panic!("'{}' did not build: {error:?}", definition.id));
            assert_eq!(provider.id(), definition.id);
            // Bearer must work exactly when the capability admits it.
            let bearer_result = build_durable_provider(
                definition.id,
                bearer("catalog-bearer"),
                base_url,
                "Catalog",
            );
            assert_eq!(
                bearer_result.is_ok(),
                definition
                    .credential_capability
                    .accepts(CredentialKind::BearerToken),
                "'{}' bearer disposition drifted",
                definition.id
            );
        }
    }

    #[test]
    fn specialized_builders_keep_their_implementation_identity() {
        let cases = [
            ("xai", "xai"),
            ("opencode_go", "opencode_go"),
            ("minimax", "minimax"),
            ("openrouter", "openrouter"),
            ("opencode_zen", "opencode_zen"),
            ("mistral", "mistral"),
            ("anthropic", "anthropic"),
            ("openai", "openai"),
            ("google", "google"),
        ];
        for (provider_id, expected_id) in cases {
            let provider =
                build_durable_provider(provider_id, api_key("identity-key"), None, "Identity")
                    .expect("specialized build succeeds");
            assert_eq!(provider.id(), expected_id);
        }
        let azure = build_durable_provider(
            "azure",
            api_key("identity-key"),
            Some("https://azure.example"),
            "Azure",
        )
        .expect("azure builds with an endpoint");
        assert_eq!(azure.id(), "azure");
        let eggpool = build_durable_provider(
            "eggpool",
            api_key("identity-key"),
            Some("https://eggpool.example/v1"),
            "Eggpool",
        )
        .expect("eggpool preset builds with an endpoint");
        assert_eq!(eggpool.id(), "eggpool");
    }

    #[test]
    fn unknown_compatible_preset_keeps_generic_transport_with_bearer() {
        let provider = build_durable_provider(
            "gateway",
            bearer("gateway-bearer"),
            Some("https://gateway.example/v1"),
            "Gateway",
        )
        .expect("unknown preset with an endpoint stays buildable");
        assert_eq!(provider.id(), "gateway");
        assert!(
            build_durable_provider("gateway", api_key("key"), None, "Gateway").is_err(),
            "unknown preset without an endpoint must fail"
        );
    }

    #[test]
    fn incompatible_credential_kind_fails_without_leaking_the_secret() {
        let error =
            match build_durable_provider("anthropic", bearer("super-secret-bearer"), None, "") {
                Ok(_) => panic!("bearer must be rejected for an API-key-only provider"),
                Err(error) => error,
            };
        assert!(matches!(
            error,
            ConnectionError::UnsupportedCredentialKind { .. }
        ));
        assert!(!format!("{error:?}").contains("super-secret-bearer"));
        assert!(!error.to_string().contains("super-secret-bearer"));
    }

    #[test]
    fn endpoint_requiring_builders_reject_missing_endpoints_without_secrets() {
        for provider_id in ["azure", "custom", "eggpool"] {
            let error = match build_durable_provider(provider_id, api_key("endpoint-key"), None, "")
            {
                Ok(_) => panic!("endpoint-requiring build must fail without a base URL"),
                Err(error) => error,
            };
            assert!(
                matches!(error, ConnectionError::InvalidDescriptor(_)),
                "unexpected error for '{provider_id}': {error:?}"
            );
            assert!(!error.to_string().contains("endpoint-key"));
        }
    }

    #[test]
    fn catalog_metadata_is_secret_free_by_construction() {
        // Definitions never hold credential values; the debug projection
        // must stay free of secret-shaped material.
        let debug = format!("{:?}", provider_setup_catalog());
        assert!(!debug.contains("sk-"));
        assert!(!debug.contains("Bearer "));
        for definition in provider_setup_catalog() {
            assert!(!definition.id.trim().is_empty());
            assert!(!definition.display_name.trim().is_empty());
            assert!(!definition.description.trim().is_empty());
        }
    }

    #[test]
    fn opencode_go_base_url_mirrors_the_shared_profile() {
        // The shared provider-profile contract owns the endpoint. The durable
        // connection record mirrors it because SetupEndpointPolicy::Fixed
        // stores a &'static str; if these ever diverge, a stored connection
        // would name a base URL the runtime provider does not use.
        let shared = crate::provider_profile::shared_base_url("opencode_go")
            .expect("shared profile owns the OpenCode Go base URL");
        assert_eq!(OPENCODE_GO_BASE_URL, shared);
        assert_eq!(
            crate::opencode_go::shared_base_url().as_deref(),
            Some(shared.as_str())
        );
    }

    #[test]
    fn opencode_go_uses_current_first_party_zen_prefix() {
        // First-party OpenCode Go docs (reviewed 2026-10-02) serve Chat,
        // Responses, Anthropic Messages, and /models under
        // https://opencode.ai/zen/go/v1.
        assert_eq!(OPENCODE_GO_BASE_URL, "https://opencode.ai/zen/go/v1");
        let definition = setup_definition("opencode_go").expect("opencode_go defined");
        assert_eq!(
            definition.fixed_base_url(),
            Some("https://opencode.ai/zen/go/v1")
        );
    }

    #[test]
    fn together_endpoint_remains_on_documented_xyz_prefix() {
        // Together serves the OpenAI-compatible API under both
        // https://api.together.xyz/v1 (legacy, retained) and
        // https://api.together.ai/v1 (current canonical). CodeGG retains
        // the documented `.xyz` prefix to avoid silently repointing stored
        // fixed endpoints; no migration is authorized by C002.
        assert_eq!(TOGETHER_BASE_URL, "https://api.together.xyz/v1");
        let definition = setup_definition("together").expect("together defined");
        assert_eq!(
            definition.fixed_base_url(),
            Some("https://api.together.xyz/v1")
        );
    }

    #[test]
    fn fixed_endpoints_cannot_be_silently_repointed() {
        // Fixed definitions win over caller-supplied endpoints: a hand-built
        // descriptor cannot repoint a fixed upstream.
        let provider = build_durable_provider(
            "together",
            api_key("fixed-key"),
            Some("https://evil.example/v1"),
            "Together",
        )
        .expect("fixed build succeeds");
        assert_eq!(provider.id(), "together");
        // The durable builder for OpenAiCompatibleFixed ignores the caller
        // URL by construction (uses the catalog constant). Prove the catalog
        // constant is the authority.
        assert_eq!(
            setup_definition("together")
                .expect("together defined")
                .fixed_base_url(),
            Some(TOGETHER_BASE_URL)
        );
        // OpenCode Go affinity builder also uses the catalog constant.
        let go = build_durable_provider(
            "opencode_go",
            api_key("go-key"),
            Some("https://evil.example/v1"),
            "Go",
        )
        .expect("opencode_go builds");
        assert_eq!(go.id(), "opencode_go");
    }

    #[test]
    fn every_fixed_definition_has_a_non_empty_http_prefix() {
        for definition in provider_setup_catalog() {
            if let Some(base_url) = definition.fixed_base_url() {
                assert!(
                    base_url.starts_with("https://"),
                    "'{}' fixed endpoint must be https: {base_url}",
                    definition.id
                );
                assert!(
                    !base_url.ends_with('/'),
                    "'{}' fixed endpoint must not end with '/': {base_url}",
                    definition.id
                );
            }
        }
    }
}
