//! Durable, revision-scoped writer for provider connection credential
//! qualification (M010).
//!
//! Catalog discovery can never validate a credential for most providers, so
//! the first real verdict comes from an authenticated inference request. This
//! module owns writing that verdict back to
//! `provider_connection_health.credential_status`.
//!
//! Rules, all enforced by SQL rather than by caller discipline:
//!
//! - **Revision scoped.** Every write carries `WHERE connection_id = ? AND
//!   revision = ?`. A verdict for a credential that has since been rotated
//!   matches no row and is discarded, so a late-arriving turn can never mark
//!   a *new* credential as bad.
//! - **Inconclusive writes nothing.** Transient transport failures, timeouts,
//!   rate limits, and cancellations produce no row update at all, so a flaky
//!   network cannot erase a genuine verdict.
//! - **Monotonic in safety.** `authentication_failed` only comes from a typed
//!   401/403. `verified` only comes from a completed authenticated request.
//! - **Secret free.** Only the connection id, revision, and a typed outcome
//!   cross this boundary; no credential material is ever read here.

use std::sync::Arc;

pub use crate::agent::provider_qualification::InferenceCredentialOutcome;
use crate::agent::provider_qualification::ProviderCredentialObserver;
use codegg_providers::qualification::CredentialVerification;

/// Stable reason code recorded alongside a genuine credential rejection.
pub const CREDENTIAL_AUTH_FAILURE_REASON: &str = "credential_authentication_failed";

/// Records inference outcomes against one concrete connection revision.
#[derive(Debug, Clone)]
pub struct ProviderConnectionCredentialReporter {
    pool: sqlx::SqlitePool,
    connection_id: Arc<str>,
    revision: i64,
}

impl ProviderConnectionCredentialReporter {
    pub fn new(pool: sqlx::SqlitePool, connection_id: Arc<str>, revision: u64) -> Self {
        Self {
            pool,
            connection_id,
            revision: i64::try_from(revision).unwrap_or(i64::MAX),
        }
    }

    /// Apply one outcome. Returns `true` when a row was actually updated,
    /// which is `false` for a stale revision and for inconclusive outcomes.
    pub async fn record(&self, outcome: InferenceCredentialOutcome) -> bool {
        // Transient / non-auth failures deliberately produce no SQL at all.
        let (credential_status, health_status, reason_code) = match outcome {
            InferenceCredentialOutcome::Authenticated => {
                (CredentialVerification::Verified.code(), "healthy", None)
            }
            InferenceCredentialOutcome::AuthenticationFailed => (
                CredentialVerification::AuthenticationFailed.code(),
                "unhealthy",
                Some(CREDENTIAL_AUTH_FAILURE_REASON),
            ),
            InferenceCredentialOutcome::Inconclusive => return false,
        };
        let result = sqlx::query(
            "UPDATE provider_connection_health \
             SET credential_status = ?, status = ?, reason_code = ?, checked_at = ? \
             WHERE connection_id = ? AND revision = ?",
        )
        .bind(credential_status)
        .bind(health_status)
        .bind(reason_code)
        .bind(now_millis())
        .bind(self.connection_id.as_ref())
        .bind(self.revision)
        .execute(&self.pool)
        .await;
        match result {
            Ok(update) => {
                let updated = update.rows_affected() > 0;
                if !updated {
                    tracing::debug!(
                        connection_id = %self.connection_id,
                        revision = self.revision,
                        outcome = ?outcome,
                        "credential outcome discarded: connection revision moved on"
                    );
                }
                updated
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    connection_id = %self.connection_id,
                    revision = self.revision,
                    "provider connection credential write failed"
                );
                false
            }
        }
    }
}

impl ProviderCredentialObserver for ProviderConnectionCredentialReporter {
    /// Non-blocking by contract: the turn must not wait on storage. The write
    /// is idempotent and revision-scoped, so a dropped task only loses a
    /// health hint, never durable state.
    fn observe(&self, outcome: InferenceCredentialOutcome) {
        if outcome == InferenceCredentialOutcome::Inconclusive {
            return;
        }
        let reporter = self.clone();
        tokio::spawn(async move {
            reporter.record(outcome).await;
        });
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| i64::try_from(value.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
