//! Narrow adapter over the shared EggPool provider-profile contract.
//!
//! EggPool's `eggpool-provider-profile` crate owns the secret-free, sans-I/O
//! provider metadata contract and the single canonical bundled profile asset.
//! CodeGG consumes it at the pinned EggPool M001 closure revision as the shared
//! metadata source for providers represented in both repositories, rather than
//! keeping a second, contradictory copy of the same endpoint/path/auth facts.
//!
//! Deliberate boundaries:
//!
//! * No EggPool root config/runtime types cross this boundary. Only the profile
//!   contract's neutral types are projected into CodeGG-shaped values.
//! * The shared profile carries **no secrets** and never owns credentials.
//!   CodeGG retains credential values, secret references, HTTP transport,
//!   cancellation, and retry/error ownership.
//! * The adapter is pure resolution. It performs no I/O, reads no environment,
//!   and holds no state beyond the parsed embedded registry.
//!
//! A model with no reviewed wire hint resolves to
//! [`ProfileError::WireUnresolved`]. Resolution never falls back to Chat
//! Completions and never guesses from a model prefix or family; failing closed
//! is the contract, not a fallback tier.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use eggpool_provider_profile::{
    ProviderModelsEndpointProfile, ProviderProfileRegistry, WireSurface,
};

/// CodeGG provider id -> shared EggPool profile id.
///
/// Durable CodeGG ids are never renamed to match the sibling repository's
/// spelling. CodeGG uses local underscore conventions (`opencode_go`) while the
/// shared asset uses hyphens (`opencode-go`); this explicit map is the whole
/// reconciliation. Ids absent from this map are deliberately not adapted.
const SHARED_IDS: &[(&str, &str)] = &[("opencode_go", "opencode-go")];

/// CodeGG provider id -> shared EggPool profile id, for **model discovery only**.
///
/// Deliberately a separate, weaker map than [`SHARED_IDS`]. Adapting a provider
/// for *wire routing* asserts that CodeGG may execute a request against the
/// resolved surface and path, so that map is intentionally narrow. Adapting a
/// provider for *discovery* only asserts where the reviewed `/models` endpoint
/// lives, which is a far smaller claim, and it is what lets CodeGG stop
/// hardcoding `{base_url}/models` as a second owner for the same fact.
///
/// Keeping the two maps apart means widening discovery never silently widens
/// routing authority.
const DISCOVERY_IDS: &[(&str, &str)] = &[
    ("opencode_go", "opencode-go"),
    ("openai", "openai"),
    ("anthropic", "anthropic"),
    ("openrouter", "openrouter"),
    // CodeGG's `google` provider serves the native `generateContent` surface at
    // `https://generativelanguage.googleapis.com/v1beta`, which is exactly the
    // shared `gemini-native` profile's base URL, wire surface, and `/models`
    // endpoint. The shared asset spells this provider `gemini-native`, so the
    // reconciliation is explicit rather than inferred.
    ("google", "gemini-native"),
];

/// How the resolved surface carries the credential.
///
/// The credential *value* is always supplied by CodeGG at request time. This
/// type only describes the shape the shared profile declares for the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAuth {
    /// `Authorization: Bearer <credential>`.
    Bearer {
        /// Header carrying the scheme + credential.
        header: String,
        /// Scheme token, conventionally `Bearer`.
        scheme: String,
    },
    /// A bare credential header, conventionally `x-api-key`.
    ApiKeyHeader {
        /// Header carrying the credential verbatim.
        header: String,
    },
    /// The credential is sent verbatim in `Authorization`.
    RawAuthorization { header: String },
    /// This surface declares no credential.
    None,
}

/// A fully resolved, executable route for exactly one model on one provider.
///
/// A route exists only when the shared profile resolves the model to a wire
/// surface *and* that surface to a path and auth shape. Callers must send a
/// request only to this resolved surface.
#[derive(Debug, Clone)]
pub struct SurfaceRoute {
    /// CodeGG provider id this route was resolved for.
    pub provider_id: String,
    /// The model that resolved.
    pub model: String,
    /// The exact wire surface the shared profile resolved.
    pub surface: WireSurface,
    /// Absolute request URL for this surface.
    pub url: String,
    /// Credential shape for this surface.
    pub auth: RouteAuth,
    /// Surface-scoped static headers, never credential or session headers.
    pub static_headers: Vec<(String, String)>,
    /// Whether the shared profile marks the hint `fixed`.
    ///
    /// CodeGG performs no wire negotiation, so this is diagnostic context only:
    /// a `fixed` hint and a non-fixed hint are both executed as resolved.
    pub fixed_hint: bool,
}

/// Failure to resolve an executable route from the shared profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// The CodeGG provider id has no entry in the explicit shared-id map.
    ProviderNotAdapted { provider_id: String },
    /// The shared registry does not contain the mapped provider.
    ProviderUnknown {
        provider_id: String,
        shared_id: String,
    },
    /// The model has no reviewed wire hint. Never defaulted to a surface.
    WireUnresolved { provider_id: String, model: String },
    /// The hinted surface has no path/auth shape in the profile, so the route
    /// is not executable even though the model itself is known.
    SurfaceUnavailable {
        provider_id: String,
        model: String,
        surface: WireSurface,
    },
    /// The embedded contract failed to parse or validate.
    Contract(String),
}

impl ProfileError {
    /// Stable, secret-free diagnostic code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProviderNotAdapted { .. } => "provider_profile_not_adapted",
            Self::ProviderUnknown { .. } => "provider_profile_unknown",
            Self::WireUnresolved { .. } => "wire_unresolved",
            Self::SurfaceUnavailable { .. } => "wire_surface_unavailable",
            Self::Contract(_) => "provider_profile_contract_invalid",
        }
    }
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderNotAdapted { provider_id } => {
                write!(
                    f,
                    "provider {provider_id:?} has no shared-profile adapter entry"
                )
            }
            Self::ProviderUnknown {
                provider_id,
                shared_id,
            } => write!(
                f,
                "shared provider profile {shared_id:?} (CodeGG id {provider_id:?}) is absent"
            ),
            Self::WireUnresolved { provider_id, model } => write!(
                f,
                "model {model:?} on provider {provider_id:?} has no reviewed wire mapping"
            ),
            Self::SurfaceUnavailable {
                provider_id,
                model,
                surface,
            } => write!(
                f,
                "model {model:?} on provider {provider_id:?} maps to {surface:?}, \
                 which the shared profile does not serve"
            ),
            Self::Contract(detail) => write!(f, "shared provider profile is invalid: {detail}"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// The parsed embedded shared registry, loaded once per process.
///
/// `embedded()` is a pure function over compile-time data, so malformed bundled
/// data fails here at first use rather than at dispatch.
fn registry() -> Result<&'static ProviderProfileRegistry, ProfileError> {
    static REGISTRY: OnceLock<Result<ProviderProfileRegistry, String>> = OnceLock::new();
    let cell = REGISTRY
        .get_or_init(|| ProviderProfileRegistry::embedded().map_err(|error| error.to_string()));
    match cell {
        Ok(registry) => Ok(registry),
        Err(detail) => Err(ProfileError::Contract(detail.clone())),
    }
}

/// Map a durable CodeGG provider id to its shared profile id.
pub fn shared_provider_id(provider_id: &str) -> Option<&'static str> {
    SHARED_IDS
        .iter()
        .find(|(code, _)| *code == provider_id)
        .map(|(_, shared)| *shared)
}

fn profile(
    provider_id: &str,
) -> Result<
    (
        &'static str,
        &'static eggpool_provider_profile::ProviderProfile,
    ),
    ProfileError,
> {
    let shared_id =
        shared_provider_id(provider_id).ok_or_else(|| ProfileError::ProviderNotAdapted {
            provider_id: provider_id.to_string(),
        })?;
    let profiles = registry()?;
    let profile = profiles
        .get(shared_id)
        .ok_or_else(|| ProfileError::ProviderUnknown {
            provider_id: provider_id.to_string(),
            shared_id: shared_id.to_string(),
        })?;
    Ok((shared_id, profile))
}

/// Base URL the shared profile owns for a CodeGG provider id.
///
/// Returns `None` for ids outside the explicit adapter map rather than falling
/// back to a CodeGG-local constant, so a shared-profile gap is visible instead
/// of silently reintroducing a second owner for the same fact.
pub fn shared_base_url(provider_id: &str) -> Option<String> {
    let (_, profile) = profile(provider_id).ok()?;
    Some(profile.base_url.clone())
}

/// Resolve the executable route for one model.
///
/// Fails closed. There is deliberately no Chat Completions default and no
/// prefix/family inference anywhere in this path.
pub fn resolve_route(provider_id: &str, model: &str) -> Result<SurfaceRoute, ProfileError> {
    let (_, profile) = profile(provider_id)?;

    // Exact reviewed hint only. A miss is unresolved, not a fallback.
    let preference =
        profile
            .model_wire_preference(model)
            .ok_or_else(|| ProfileError::WireUnresolved {
                provider_id: provider_id.to_string(),
                model: model.to_string(),
            })?;
    let surface = preference.preferred_surface;

    let url = profile
        .surface_url(surface)
        .ok_or_else(|| ProfileError::SurfaceUnavailable {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
            surface,
        })?;
    let auth_profile =
        profile
            .surface_auth(surface)
            .ok_or_else(|| ProfileError::SurfaceUnavailable {
                provider_id: provider_id.to_string(),
                model: model.to_string(),
                surface,
            })?;

    use eggpool_provider_profile::ProviderAuthMode;
    let auth = match auth_profile.mode {
        ProviderAuthMode::Bearer => RouteAuth::Bearer {
            header: auth_profile.header.clone(),
            scheme: if auth_profile.scheme.is_empty() {
                "Bearer".to_string()
            } else {
                auth_profile.scheme.clone()
            },
        },
        ProviderAuthMode::ApiKey => RouteAuth::ApiKeyHeader {
            header: auth_profile.header.clone(),
        },
        ProviderAuthMode::RawAuthorization => RouteAuth::RawAuthorization {
            header: auth_profile.header.clone(),
        },
        ProviderAuthMode::None => RouteAuth::None,
    };

    let static_headers = profile
        .wire_surfaces
        .get(&surface)
        .map(|entry| {
            entry
                .headers
                .iter()
                .filter_map(|header| {
                    header
                        .value
                        .as_ref()
                        .map(|value| (header.name.clone(), value.clone()))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(SurfaceRoute {
        provider_id: provider_id.to_string(),
        model: model.to_string(),
        surface,
        url,
        auth,
        static_headers,
        fixed_hint: preference.fixed,
    })
}

/// Whether the shared profile resolves this model to an executable surface.
pub fn is_wire_resolved(provider_id: &str, model: &str) -> bool {
    resolve_route(provider_id, model).is_ok()
}

/// Every model id the shared profile resolves to an executable surface.
///
/// Used to qualify discovered catalogs: a model outside this set is wire
/// unresolved and must not be advertised as ordinarily selectable.
pub fn wire_resolved_models(provider_id: &str) -> BTreeSet<String> {
    let Ok((_, profile)) = profile(provider_id) else {
        return BTreeSet::new();
    };
    profile
        .model_wire
        .iter()
        .filter(|(_, preference)| profile.surface_url(preference.preferred_surface).is_some())
        .map(|(model, _)| model.clone())
        .collect()
}

/// Every provider id this adapter maps into the shared contract.
pub fn adapted_provider_ids() -> impl Iterator<Item = &'static str> {
    SHARED_IDS.iter().map(|(code, _)| *code)
}

/// Shared profile id for a CodeGG provider id, for discovery only.
fn discovery_shared_id(provider_id: &str) -> Option<&'static str> {
    DISCOVERY_IDS
        .iter()
        .find(|(code, _)| *code == provider_id)
        .map(|(_, shared)| *shared)
}

/// The reviewed model-discovery endpoint for a CodeGG provider id.
///
/// Method, path, and query come from the shared profile's
/// [`ProviderProfile::resolved_models_endpoint`], so a provider's discovery
/// contract is data CodeGG consumes rather than a CodeGG-local URL constant.
/// `required` reports whether the profile marks discovery as a precondition
/// for the provider being usable.
///
/// Returns [`ProfileError::ProviderNotAdapted`] for ids outside
/// [`DISCOVERY_IDS`]. Callers must treat that as "no reviewed discovery
/// contract" rather than silently inventing one.
pub fn resolved_models_endpoint(
    provider_id: &str,
) -> Result<ProviderModelsEndpointProfile, ProfileError> {
    let shared_id =
        discovery_shared_id(provider_id).ok_or_else(|| ProfileError::ProviderNotAdapted {
            provider_id: provider_id.to_string(),
        })?;
    let profiles = registry()?;
    let profile = profiles
        .get(shared_id)
        .ok_or_else(|| ProfileError::ProviderUnknown {
            provider_id: provider_id.to_string(),
            shared_id: shared_id.to_string(),
        })?;
    Ok(profile.resolved_models_endpoint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_go_base_url_comes_from_the_shared_profile() {
        assert_eq!(
            shared_base_url("opencode_go").as_deref(),
            Some("https://opencode.ai/zen/go/v1")
        );
    }

    #[test]
    fn unadapted_provider_ids_have_no_shared_base_url() {
        // A provider outside the explicit map yields None rather than a
        // CodeGG-local constant, so there is no second owner for the fact.
        assert_eq!(shared_base_url("openai"), None);
        assert!(matches!(
            resolve_route("openai", "gpt-5"),
            Err(ProfileError::ProviderNotAdapted { .. })
        ));
    }

    #[test]
    fn representative_models_resolve_to_documented_surfaces() {
        let cases = [
            ("gpt-6-luna", WireSurface::OpenaiResponses),
            ("gpt-5.6-luna", WireSurface::OpenaiResponses),
            ("grok-4.7", WireSurface::OpenaiResponses),
            ("muse-spark-1.3-contributor", WireSurface::OpenaiResponses),
            ("glm-5.3-flash", WireSurface::OpenaiChatCompletions),
            ("minimax-m3", WireSurface::AnthropicMessages),
        ];
        for (model, expected) in cases {
            let route = resolve_route("opencode_go", model).expect("reviewed model resolves");
            assert_eq!(route.surface, expected, "model {model}");
        }
    }

    #[test]
    fn per_surface_urls_match_first_party_paths() {
        let responses = resolve_route("opencode_go", "gpt-6-luna").unwrap();
        assert_eq!(responses.url, "https://opencode.ai/zen/go/v1/responses");
        let chat = resolve_route("opencode_go", "glm-5.3-flash").unwrap();
        assert_eq!(chat.url, "https://opencode.ai/zen/go/v1/chat/completions");
        let messages = resolve_route("opencode_go", "minimax-m3").unwrap();
        assert_eq!(messages.url, "https://opencode.ai/zen/go/v1/messages");
    }

    #[test]
    fn per_surface_auth_is_profile_owned() {
        let chat = resolve_route("opencode_go", "glm-5.3-flash").unwrap();
        assert!(matches!(chat.auth, RouteAuth::Bearer { .. }));
        let responses = resolve_route("opencode_go", "gpt-6-luna").unwrap();
        assert!(matches!(responses.auth, RouteAuth::Bearer { .. }));
        let messages = resolve_route("opencode_go", "minimax-m3").unwrap();
        assert_eq!(
            messages.auth,
            RouteAuth::ApiKeyHeader {
                header: "x-api-key".to_string()
            }
        );
    }

    #[test]
    fn unknown_model_is_unresolved_and_never_defaults_to_chat() {
        let error = resolve_route("opencode_go", "totally-unknown-model").unwrap_err();
        assert_eq!(error.code(), "wire_unresolved");
        assert!(matches!(error, ProfileError::WireUnresolved { .. }));
        assert!(!is_wire_resolved("opencode_go", "totally-unknown-model"));
    }

    #[test]
    fn unknown_model_is_not_retried_against_a_neighbouring_family() {
        // A near-miss must not inherit a reviewed sibling's surface.
        assert!(!is_wire_resolved("opencode_go", "gpt-6-luna-preview"));
        assert!(!is_wire_resolved("opencode_go", "minimax"));
    }

    #[test]
    fn wire_resolved_set_excludes_unknown_and_includes_reviewed() {
        let resolved = wire_resolved_models("opencode_go");
        assert!(resolved.contains("gpt-6-luna"));
        assert!(resolved.contains("glm-5.3-flash"));
        assert!(resolved.contains("minimax-m3"));
        assert!(!resolved.contains("some-future-unreviewed-model"));
        assert_eq!(
            resolved.len(),
            profile("opencode_go").unwrap().1.model_wire.len()
        );
    }

    #[test]
    fn unadapted_provider_yields_an_empty_resolved_set() {
        assert!(wire_resolved_models("openai").is_empty());
    }

    #[test]
    fn every_shared_hint_is_non_fixed_and_reported_as_advisory() {
        for model in wire_resolved_models("opencode_go") {
            let route = resolve_route("opencode_go", &model).unwrap();
            assert!(
                !route.fixed_hint,
                "the reviewed Go table must stay non-fixed for {model}"
            );
        }
    }

    #[test]
    fn profile_resolution_uses_the_single_shared_wire_vocabulary() {
        // The whole point of the aligned pin: the profile's WireSurface is the
        // same type codegg-providers already encodes with.
        let route = resolve_route("opencode_go", "gpt-6-luna").unwrap();
        let as_wire: eggpool_wire::profile::WireSurface = route.surface;
        assert_eq!(as_wire, eggpool_wire::profile::WireSurface::OpenaiResponses);
    }

    #[test]
    fn adapted_provider_ids_are_explicit() {
        let ids: Vec<_> = adapted_provider_ids().collect();
        assert!(ids.contains(&"opencode_go"));
        // Durable CodeGG spelling is preserved, never rewritten to the sibling's.
        assert!(!ids.contains(&"opencode-go"));
    }

    #[test]
    fn discovery_endpoints_resolve_from_the_shared_profile() {
        // Method/path/required are profile data, so a reviewed provider
        // resolves without CodeGG owning a local URL constant.
        let endpoint = resolved_models_endpoint("opencode_go").expect("reviewed provider resolves");
        assert_eq!(endpoint.method, "GET");
        assert_eq!(endpoint.path, "/models");
        assert!(endpoint.required, "the Go profile marks discovery required");

        let anthropic = resolved_models_endpoint("anthropic").expect("anthropic resolves");
        assert_eq!(anthropic.path, "/models");
        assert!(
            !anthropic.required,
            "the Anthropic profile marks discovery optional"
        );
    }

    #[test]
    fn unadapted_provider_has_no_reviewed_discovery_contract() {
        // Failing closed is what lets a caller tell "no reviewed endpoint"
        // apart from "endpoint is /models".
        assert!(matches!(
            resolved_models_endpoint("not-a-real-provider"),
            Err(ProfileError::ProviderNotAdapted { .. })
        ));
    }

    #[test]
    fn discovery_adaptation_never_widens_wire_routing_authority() {
        // Discovery is a much weaker claim than routing. A provider may have a
        // reviewed `/models` endpoint while CodeGG still refuses to resolve its
        // wire surface, so the two maps must stay independent.
        assert!(discovery_shared_id("openai").is_some());
        assert!(
            shared_provider_id("openai").is_none(),
            "adapting discovery must not adapt routing"
        );
        assert!(wire_resolved_models("openai").is_empty());
    }
}
