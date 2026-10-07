//! Provider connection qualification semantics.
//!
//! Catalog discovery and credential verification are **separate** facts about
//! a connection, and this module keeps them separate in the type system.
//!
//! The defect this replaces: provisioning treated a successful
//! [`crate::Provider::models`] call as proof that a credential was accepted.
//! That is false for every provider whose `models()` returns a local/static
//! array (no network I/O at all), for best-effort OpenAI-compatible discovery
//! that intentionally falls back instead of proving auth, and for any provider
//! whose `/models` endpoint is publicly readable. The result was both
//! false-positive connections ("verified" for a garbage key) and false-negative
//! ones.
//!
//! The two axes:
//!
//! - [`CatalogOutcome`] — can the provider enumerate models right now? This is
//!   a transport/catalog fact and says nothing about the credential.
//! - [`CredentialVerification`] — has an *authenticated* operation actually
//!   accepted this credential? Only [`CredentialEvidence::Authenticated`]
//!   strategies can answer yes, and only real inference can promote later.
//!
//! Invariants enforced here (and re-checked by
//! `scripts/check_provider_qualification.py`):
//!
//! 1. A successful catalog probe never by itself yields
//!    [`CredentialVerification::Verified`].
//! 2. A catalog/transport failure is never reported as an authentication
//!    rejection.
//! 3. No provider is ever billed to validate a credential: nothing in this
//!    module issues an inference request.

use crate::error::ProviderError;
use serde::{Deserialize, Serialize};

/// What a probe strategy is allowed to prove about a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialEvidence {
    /// The probe only enumerates a catalog. Success means "models were
    /// listed", never "this key was accepted". Covers local/static model
    /// arrays, best-effort discovery, and publicly readable catalogs.
    CatalogOnly,
    /// The probe is a genuinely authenticated, non-billable metadata request:
    /// the endpoint itself validated the presented credential. A 2xx is proof
    /// the credential was accepted.
    Authenticated,
}

impl CredentialEvidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CatalogOnly => "catalog_only",
            Self::Authenticated => "authenticated",
        }
    }
}

/// Why bounded catalog discovery could not reach a usable model list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogUnavailable {
    /// DNS/connect/route failure. Transient by nature.
    Unreachable,
    /// The bounded request deadline elapsed. Transient.
    Timeout,
    /// TLS negotiation or certificate failure.
    TlsFailed,
    /// The operation was cancelled by the caller.
    Cancelled,
}

impl CatalogUnavailable {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unreachable => "endpoint_unreachable",
            Self::Timeout => "probe_timeout",
            Self::TlsFailed => "tls_failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Transient means "the same request may succeed later". None of these are
    /// evidence about the credential.
    pub const fn is_transient(self) -> bool {
        !matches!(self, Self::TlsFailed)
    }
}

/// Why a reachable catalog response was rejected as unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRejected {
    /// The body was not the expected model-list shape.
    InvalidJson,
    /// The response exceeded the bounded size/model-count limits.
    Oversized,
    /// The response was well-formed but contained no usable model IDs.
    Empty,
    /// The endpoint does not expose the catalog API CodeGG expects.
    UnsupportedApi,
}

impl CatalogRejected {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid_json",
            Self::Oversized => "catalog_oversized",
            Self::Empty => "empty_catalog",
            Self::UnsupportedApi => "unsupported_api",
        }
    }
}

/// Result of bounded catalog discovery, independent of authentication.
///
/// This type deliberately has **no** authentication variant. A public catalog
/// that answers 200 proves reachability, not credential validity, so that
/// distinction cannot be expressed here at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CatalogOutcome {
    /// A bounded, usable model list was produced.
    Available {
        model_count: usize,
        /// Digest identifying the discovered catalog contents.
        revision: String,
        duration_ms: u64,
    },
    /// The catalog could not be reached within bounds.
    Unavailable {
        reason: CatalogUnavailable,
        reason_code: String,
    },
    /// The catalog was reached but its response was unusable.
    Rejected {
        reason: CatalogRejected,
        reason_code: String,
    },
}

impl CatalogOutcome {
    pub fn available(model_count: usize, revision: impl Into<String>, duration_ms: u64) -> Self {
        Self::Available {
            model_count,
            revision: revision.into(),
            duration_ms,
        }
    }

    pub fn unavailable(reason: CatalogUnavailable) -> Self {
        Self::Unavailable {
            reason,
            reason_code: reason.code().to_owned(),
        }
    }

    pub fn rejected(reason: CatalogRejected) -> Self {
        Self::Rejected {
            reason,
            reason_code: reason.code().to_owned(),
        }
    }

    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Stable, secret-safe reason code. `None` when the catalog is available.
    pub fn reason_code(&self) -> Option<&str> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable { reason_code, .. } | Self::Rejected { reason_code, .. } => {
                Some(reason_code)
            }
        }
    }

    /// Whether a failure here says anything about the credential. It never
    /// does: catalog and transport failures are not authentication evidence.
    pub fn is_authentication_rejection(&self) -> bool {
        false
    }
}

/// Credential verification state for one connection revision.
///
/// `Unverified` is a first-class, durable state — not an error and not a
/// synonym for "healthy". It means "configured, and no authenticated
/// operation has accepted this credential yet".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialVerification {
    /// An authenticated operation accepted this exact credential.
    Verified,
    /// Configured, but no authenticated operation has accepted it yet.
    Unverified,
    /// An authenticated operation rejected this credential (401/403).
    AuthenticationFailed,
    /// The provider needs no credential, so verification does not apply.
    NoCredentialRequired,
}

impl CredentialVerification {
    /// Durable storage / wire representation. These strings are part of the
    /// persisted contract and must stay in sync with the
    /// `provider_connection_health.credential_status` CHECK constraint.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
            Self::AuthenticationFailed => "authentication_failed",
            Self::NoCredentialRequired => "no_credential_required",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "verified" => Some(Self::Verified),
            "unverified" => Some(Self::Unverified),
            "authentication_failed" => Some(Self::AuthenticationFailed),
            "no_credential_required" => Some(Self::NoCredentialRequired),
            _ => None,
        }
    }

    /// Whether the credential has positively been proven usable.
    pub const fn is_verified(self) -> bool {
        matches!(self, Self::Verified)
    }

    /// Whether an authenticated operation rejected this credential.
    pub const fn is_authentication_failed(self) -> bool {
        matches!(self, Self::AuthenticationFailed)
    }

    /// Derive the credential state produced by one provisioning probe.
    ///
    /// This is the **only** place provisioning may obtain a credential state
    /// from a probe, which is what makes invariant 1 structural rather than a
    /// convention:
    ///
    /// - `CatalogOnly` + catalog available → [`Self::Unverified`], never
    ///   `Verified`. A static/local model list or a publicly readable
    ///   `/models` endpoint proves nothing about the credential.
    /// - `Authenticated` + catalog available → [`Self::Verified`]; the
    ///   endpoint itself validated the presented credential.
    /// - Either evidence + an authentication rejection →
    ///   [`Self::AuthenticationFailed`].
    /// - Either evidence + any non-authentication failure → the connection was
    ///   never committed, so this is [`Self::Unverified`] (the caller must not
    ///   persist a credential verdict derived from a transport failure).
    pub const fn from_probe(
        evidence: CredentialEvidence,
        catalog: &CatalogOutcome,
        authentication_rejected: bool,
    ) -> Self {
        if authentication_rejected {
            return Self::AuthenticationFailed;
        }
        match (evidence, catalog.is_available()) {
            // A reachable catalog only proves the credential when the probe
            // itself was an authenticated metadata request.
            (CredentialEvidence::Authenticated, true) => Self::Verified,
            // Everything else is honestly unknown.
            (CredentialEvidence::CatalogOnly, true) => Self::Unverified,
            (CredentialEvidence::Authenticated, false) => Self::Unverified,
            (CredentialEvidence::CatalogOnly, false) => Self::Unverified,
        }
    }
}

/// The combined qualification result of one provisioning probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeQualification {
    pub catalog: CatalogOutcome,
    pub credential: CredentialVerification,
}

impl ProbeQualification {
    pub const fn new(catalog: CatalogOutcome, credential: CredentialVerification) -> Self {
        Self {
            catalog,
            credential,
        }
    }

    /// Classify one completed probe.
    ///
    /// `Err(error)` carries the provider-typed failure. Only a typed
    /// authentication rejection promotes to
    /// [`CredentialVerification::AuthenticationFailed`]; every other provider
    /// error is inconclusive and must not be reported as "invalid key".
    pub fn classify(
        evidence: CredentialEvidence,
        catalog: CatalogOutcome,
        error: Option<&ProviderError>,
    ) -> Self {
        let authentication_rejected = error.is_some_and(|error| error.error_class() == "auth");
        Self {
            credential: CredentialVerification::from_probe(
                evidence,
                &catalog,
                authentication_rejected,
            ),
            catalog,
        }
    }
}

/// Classify a provider error into the catalog axis.
///
/// Auth failures are *not* a catalog outcome: they belong to
/// [`CredentialVerification`]. Everything else is a catalog fact.
pub fn catalog_outcome_for_error(error: &ProviderError) -> CatalogOutcome {
    match error {
        ProviderError::Timeout(_) => CatalogOutcome::unavailable(CatalogUnavailable::Timeout),
        ProviderError::Transport { kind } => {
            CatalogOutcome::unavailable(transport_unavailable(kind))
        }
        ProviderError::Stream(_) => CatalogOutcome::unavailable(CatalogUnavailable::Unreachable),
        ProviderError::CircuitOpen(_) => {
            CatalogOutcome::unavailable(CatalogUnavailable::Unreachable)
        }
        ProviderError::RateLimit | ProviderError::RateLimited { .. } => {
            CatalogOutcome::unavailable(CatalogUnavailable::Timeout)
        }
        ProviderError::Api { code, .. } => {
            if code.parse::<u16>().is_ok_and(|status| status == 404) {
                CatalogOutcome::rejected(CatalogRejected::UnsupportedApi)
            } else {
                CatalogOutcome::rejected(CatalogRejected::InvalidJson)
            }
        }
        ProviderError::Auth(_) | ProviderError::ModelNotFound(_) | ProviderError::NotFound(_) => {
            CatalogOutcome::rejected(CatalogRejected::UnsupportedApi)
        }
    }
}

fn transport_unavailable(kind: &str) -> CatalogUnavailable {
    match kind {
        "timeout" | "timed_out" => CatalogUnavailable::Timeout,
        "tls" | "tls_error" | "certificate" => CatalogUnavailable::TlsFailed,
        _ => CatalogUnavailable::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available() -> CatalogOutcome {
        CatalogOutcome::available(3, "rev-1", 12)
    }

    #[test]
    fn catalog_only_success_is_never_verified() {
        assert_eq!(
            CredentialVerification::from_probe(
                CredentialEvidence::CatalogOnly,
                &available(),
                false
            ),
            CredentialVerification::Unverified
        );
    }

    #[test]
    fn authenticated_metadata_success_is_verified() {
        assert_eq!(
            CredentialVerification::from_probe(
                CredentialEvidence::Authenticated,
                &available(),
                false
            ),
            CredentialVerification::Verified
        );
    }

    #[test]
    fn authentication_rejection_wins_regardless_of_evidence() {
        for evidence in [
            CredentialEvidence::CatalogOnly,
            CredentialEvidence::Authenticated,
        ] {
            assert_eq!(
                CredentialVerification::from_probe(evidence, &available(), true),
                CredentialVerification::AuthenticationFailed
            );
        }
    }

    #[test]
    fn catalog_failure_is_never_an_authentication_verdict() {
        let failures = [
            CatalogOutcome::unavailable(CatalogUnavailable::Unreachable),
            CatalogOutcome::unavailable(CatalogUnavailable::Timeout),
            CatalogOutcome::unavailable(CatalogUnavailable::TlsFailed),
            CatalogOutcome::rejected(CatalogRejected::InvalidJson),
            CatalogOutcome::rejected(CatalogRejected::Oversized),
            CatalogOutcome::rejected(CatalogRejected::Empty),
            CatalogOutcome::rejected(CatalogRejected::UnsupportedApi),
        ];
        for catalog in failures {
            assert!(
                !catalog.is_authentication_rejection(),
                "{catalog:?} must not read as an auth rejection"
            );
            assert_eq!(
                CredentialVerification::from_probe(
                    CredentialEvidence::Authenticated,
                    &catalog,
                    false
                ),
                CredentialVerification::Unverified
            );
        }
    }

    #[test]
    fn codes_round_trip_through_the_durable_contract() {
        for state in [
            CredentialVerification::Verified,
            CredentialVerification::Unverified,
            CredentialVerification::AuthenticationFailed,
            CredentialVerification::NoCredentialRequired,
        ] {
            assert_eq!(CredentialVerification::parse(state.code()), Some(state));
        }
        assert_eq!(CredentialVerification::parse("healthy"), None);
    }

    #[test]
    fn classify_uses_typed_provider_error_classes() {
        let auth = ProviderError::from_http_status(401, "unauthorized");
        let qualification =
            ProbeQualification::classify(CredentialEvidence::CatalogOnly, available(), Some(&auth));
        assert_eq!(
            qualification.credential,
            CredentialVerification::AuthenticationFailed
        );

        // A transient failure must never be reported as an invalid key.
        for transient in [
            ProviderError::Timeout("deadline".to_string()),
            ProviderError::Transport {
                kind: "dns".to_string(),
            },
            ProviderError::from_http_status(503, "unavailable"),
        ] {
            let qualification = ProbeQualification::classify(
                CredentialEvidence::Authenticated,
                CatalogOutcome::unavailable(CatalogUnavailable::Timeout),
                Some(&transient),
            );
            assert_ne!(
                qualification.credential,
                CredentialVerification::AuthenticationFailed,
                "{transient} must not read as an invalid key"
            );
        }
    }

    #[test]
    fn catalog_outcome_mapping_keeps_transport_distinct_from_auth() {
        assert_eq!(
            catalog_outcome_for_error(&ProviderError::Timeout("x".to_string())),
            CatalogOutcome::unavailable(CatalogUnavailable::Timeout)
        );
        assert_eq!(
            catalog_outcome_for_error(&ProviderError::Transport {
                kind: "dns".to_string()
            }),
            CatalogOutcome::unavailable(CatalogUnavailable::Unreachable)
        );
        // An auth error maps to no catalog verdict at all.
        assert!(
            !catalog_outcome_for_error(&ProviderError::from_http_status(403, "no")).is_available()
        );
    }

    #[test]
    fn serialization_is_additive_and_stable() {
        let json = serde_json::to_string(&CredentialVerification::Unverified).expect("serialize");
        assert_eq!(json, "\"unverified\"");
        let catalog =
            serde_json::to_string(&CatalogOutcome::unavailable(CatalogUnavailable::Timeout))
                .expect("serialize");
        assert!(catalog.contains("probe_timeout"), "{catalog}");
        let qualification =
            ProbeQualification::classify(CredentialEvidence::CatalogOnly, available(), None);
        assert_eq!(qualification.credential, CredentialVerification::Unverified);
    }
}
