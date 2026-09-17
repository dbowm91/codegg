//! Narrow external task-trigger capability (Project Work Orders M005).
//!
//! A task trigger lets shell scripts, CI glue, cron wrappers, and other
//! automation satisfy one declared `ExternalTrigger` gate on one
//! `WorkOrder` occurrence without receiving a general CodeGG principal
//! token or broader project authority.
//!
//! ## Capability shape
//!
//! - The bearer has the form `cggtr_<trigger-id>.<secret>` where the
//!   secret is 32 CSPRNG bytes in URL-safe base64 without padding. The
//!   public locator finds the row without scanning hashes; the secret
//!   verifies against a SHA-256 verifier (same primitive posture as the
//!   personal-token digest store, without sharing its table or granting
//!   principal authority).
//! - Only the verifier persists. The plaintext is returned exactly once
//!   at creation; if lost, revoke and create a new trigger.
//! - Firing latches exactly one gate on the trigger's bound work order
//!   and wakes the M002 coordinator. It never starts an agent or session
//!   directly, never widens authority, and never returns project, prompt,
//!   session, model, or gate detail to the unaffiliated caller.
//! - `POST`-only at the HTTP seam (enforced outside this module); `GET`
//!   has zero side effect. Secrets never appear in query strings,
//!   projections, logs, audit metadata, or error bodies.
//!
//! ## Ownership
//!
//! This module owns trigger types, validation, secret
//! generation/verification, and redaction. Durable rows live in the
//! `task_trigger` / `task_trigger_receipt` tables owned by
//! [`super::store::WorkOrderService`]; the single fire transaction
//! (latch + count + receipt + audit-safe metadata) lives there so the
//! latch and its accounting commit atomically.
//!
//! Long-term references: ADR-0005 §12
//! (`plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`),
//! `plans/subsystems/project-work-orders-task-view-roadmap.md` M005.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::identity::{PrincipalId, ProjectId, TaskTriggerId, WorkOrderId};

// ── Wire format and verifier ──────────────────────────────────────────

/// Prefix for presented task-trigger bearers. Distinct from the
/// personal-token prefix (`cggt_`) so a trigger bearer can never match
/// the principal-token path and vice versa.
pub const TASK_TRIGGER_PREFIX: &str = "cggtr_";

/// Verifier algorithm/version stored alongside every trigger row.
/// SHA-256 over the high-entropy secret, hex-encoded. Versioned so a
/// future memory-hard verifier can coexist during rotation.
pub const TASK_TRIGGER_VERIFIER_VERSION: &str = "sha256-v1";

/// CSPRNG secret length in bytes (43 base64url-no-pad characters).
pub const TASK_TRIGGER_SECRET_BYTES: usize = 32;

/// Maximum presented bearer length accepted before parsing. Bounds
/// header/path scanning; legitimate bearers are far shorter.
pub const MAX_PRESENTED_TRIGGER_LEN: usize = 512;

/// Maximum `Idempotency-Key` header length for trigger fire retries.
pub const MAX_TRIGGER_IDEMPOTENCY_KEY_LEN: usize = 128;

/// Maximum triggers retained per bound work order. Bounds rows while
/// still permitting revoke-and-rotate overlap.
pub const MAX_TRIGGERS_PER_WORK_ORDER: usize = 16;

/// Maximum trigger metadata rows served by one list operation.
pub const MAX_TRIGGER_LIST_LIMIT: u32 = 100;

/// Default trigger metadata page size when the caller passes no limit.
pub const DEFAULT_TRIGGER_LIST_LIMIT: u32 = 50;

/// Upper bound for `max_fires`. Finite by construction; there is no
/// unbounded trigger.
pub const MAX_TRIGGER_MAX_FIRES: u32 = 1_000_000;

/// Maximum fire-request body bytes accepted by the HTTP seam. The
/// initial endpoint needs no body; the seam rejects anything larger
/// rather than parsing it.
pub const MAX_TRIGGER_FIRE_BODY_BYTES: usize = 4096;

// ── Lifecycle ─────────────────────────────────────────────────────────

/// Durable trigger lifecycle.
///
/// Only `Active`/`Revoked` persist. `Expired`/`Exhausted` are derived at
/// read/fire time from `expires_at_ms`/`max_fires`/`fire_count` so
/// expiry and exhaustion fail closed without a background sweeper, and
/// so restart trivially preserves them (they are functions of durable
/// counters, not of a cached flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTriggerState {
    Active,
    Revoked,
}

impl TaskTriggerState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// Effective trigger status including derived terminal conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTriggerStatus {
    Active,
    Revoked,
    Expired,
    Exhausted,
}

impl TaskTriggerStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::Exhausted => "exhausted",
        }
    }

    pub const fn is_fireable(self) -> bool {
        matches!(self, Self::Active)
    }
}

// ── Records ───────────────────────────────────────────────────────────

/// One durable task trigger: verifier-only credential plus occurrence
/// gate binding and fire bounds.
///
/// The plaintext secret is never a field: it exists only in the
/// creation response. The custom [`std::fmt::Debug`] impl omits the
/// verifier so verifiers never enter logs/events through debug
/// formatting (the hex digest is still queryable in SQL by design).
#[derive(Clone, PartialEq, Eq)]
pub struct TaskTrigger {
    pub id: TaskTriggerId,
    pub revision: u64,
    pub project_id: ProjectId,
    pub work_order_id: WorkOrderId,
    /// Opaque same-project locator matching the bound work order's
    /// `ExternalTrigger` gate `trigger_ref`.
    pub trigger_ref: String,
    pub secret_verifier_hex: String,
    pub verifier_version: String,
    pub state: TaskTriggerState,
    pub created_by: PrincipalId,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub max_fires: Option<u32>,
    pub fire_count: u64,
    pub last_fired_at_ms: Option<i64>,
}

impl std::fmt::Debug for TaskTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskTrigger")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("project_id", &self.project_id)
            .field("work_order_id", &self.work_order_id)
            .field("trigger_ref", &self.trigger_ref)
            .field("verifier_version", &self.verifier_version)
            .field("state", &self.state)
            .field("created_by", &self.created_by)
            .field("created_at_ms", &self.created_at_ms)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("max_fires", &self.max_fires)
            .field("fire_count", &self.fire_count)
            .field("last_fired_at_ms", &self.last_fired_at_ms)
            .finish_non_exhaustive()
    }
}

impl TaskTrigger {
    /// Effective status at `now_ms`: revocation is monotonic, expiry and
    /// exhaustion derive from durable bounds/counters.
    pub fn status_at(&self, now_ms: i64) -> TaskTriggerStatus {
        if self.state == TaskTriggerState::Revoked {
            return TaskTriggerStatus::Revoked;
        }
        if let Some(expires) = self.expires_at_ms {
            if now_ms >= expires {
                return TaskTriggerStatus::Expired;
            }
        }
        if let Some(max) = self.max_fires {
            if self.fire_count >= u64::from(max) {
                return TaskTriggerStatus::Exhausted;
            }
        }
        TaskTriggerStatus::Active
    }

    pub fn metadata(&self, now_ms: i64) -> TaskTriggerMetadata {
        TaskTriggerMetadata {
            trigger_id: self.id.clone(),
            project_id: self.project_id.clone(),
            work_order_id: self.work_order_id.clone(),
            trigger_ref: self.trigger_ref.clone(),
            status: self.status_at(now_ms),
            created_by: self.created_by.clone(),
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
            max_fires: self.max_fires,
            fire_count: self.fire_count,
            last_fired_at_ms: self.last_fired_at_ms,
            revision: self.revision,
        }
    }
}

/// Verifier-free trigger metadata for authorized list/get and for audit.
/// Never carries the secret or its verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTriggerMetadata {
    pub trigger_id: TaskTriggerId,
    pub project_id: ProjectId,
    pub work_order_id: WorkOrderId,
    pub trigger_ref: String,
    pub status: TaskTriggerStatus,
    pub created_by: PrincipalId,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub max_fires: Option<u32>,
    pub fire_count: u64,
    pub last_fired_at_ms: Option<i64>,
    pub revision: u64,
}

impl TaskTriggerMetadata {
    pub fn to_dto(&self) -> codegg_protocol::work_order::TaskTriggerMetadataDto {
        codegg_protocol::work_order::TaskTriggerMetadataDto {
            trigger_id: self.trigger_id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            work_order_id: self.work_order_id.as_str().to_owned(),
            trigger_ref: self.trigger_ref.clone(),
            status: self.status.as_str().to_owned(),
            created_by: self.created_by.as_str().to_owned(),
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
            max_fires: self.max_fires,
            fire_count: self.fire_count,
            last_fired_at_ms: self.last_fired_at_ms,
            revision: self.revision,
        }
    }
}

/// Validated input for creating one task trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskTrigger {
    /// `None` binds the work order's single `ExternalTrigger` gate; when
    /// the work order declares several, the caller must name one.
    pub trigger_ref: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub max_fires: Option<u32>,
    pub idempotency_key: Option<String>,
}

/// Narrow outcome of one trigger fire, safe to return to the
/// unaffiliated caller: a stable opaque receipt plus whether this fire
/// newly latched the gate. Carries no project, work-order, occurrence,
/// session, model, or gate detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FireOutcome {
    /// `true` when this fire newly latched the bound gate; `false` when
    /// the gate was already latched/running for the current occurrence
    /// (idempotent replay) or no occurrence is currently fireable
    /// (inert: cancelled/terminal/no-waiting-row, without disclosing
    /// which).
    pub latched: bool,
    /// Stable opaque fire receipt. Retried `Idempotency-Key` deliveries
    /// converge on the stored receipt.
    pub receipt_id: String,
    /// `true` when the receipt was converged from an earlier delivery
    /// rather than freshly recorded.
    pub duplicate: bool,
}

impl FireOutcome {
    /// Narrow wire status vocabulary for the unaffiliated caller.
    pub const fn status(&self) -> &'static str {
        if self.latched {
            "accepted"
        } else {
            "already_fired"
        }
    }
}

// ── Secret generation / verification ──────────────────────────────────

/// Generate one trigger bearer: fresh public locator, one-time
/// plaintext `cggtr_<id>.<secret>`, and the verifier to persist.
///
/// The secret is 32 CSPRNG bytes; only the SHA-256 verifier is stored.
pub fn generate_task_trigger() -> (TaskTriggerId, String, String) {
    let id = TaskTriggerId::new();
    let mut secret_bytes = [0u8; TASK_TRIGGER_SECRET_BYTES];
    {
        use rand::RngCore as _;
        rand::rng().fill_bytes(&mut secret_bytes);
    }
    let secret = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        secret_bytes,
    );
    let verifier_hex = task_trigger_digest_hex(&secret);
    let plaintext = format!("{TASK_TRIGGER_PREFIX}{}.{secret}", id.as_str());
    (id, plaintext, verifier_hex)
}

/// SHA-256 hex verifier for one trigger secret segment. The secret is
/// high-entropy; the verifier is safe to persist but is still omitted
/// from `Debug` output and never leaves the store except through
/// constant-time comparison.
pub fn task_trigger_digest_hex(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

/// Split one presented bearer into its public locator and secret
/// segment. Malformed bearers fail closed without distinguishing which
/// segment was wrong.
pub fn split_presented_trigger(presented: &str) -> Option<(String, String)> {
    if presented.len() > MAX_PRESENTED_TRIGGER_LEN {
        return None;
    }
    let rest = presented.strip_prefix(TASK_TRIGGER_PREFIX)?;
    let (trigger_id, secret) = rest.split_once('.')?;
    if trigger_id.is_empty() || secret.is_empty() {
        return None;
    }
    if trigger_id.len() > 128 || secret.len() > 256 {
        return None;
    }
    // The locator must satisfy the shared identity lexical contract so
    // it can never smuggle a path or control sequence into a lookup.
    TaskTriggerId::parse(trigger_id).ok()?;
    if secret.bytes().any(|b| b == 0)
        || secret.chars().any(char::is_control)
        || secret.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some((trigger_id.to_owned(), secret.to_owned()))
}

/// `true` when the bearer uses the task-trigger presentation shape and
/// therefore must verify as a trigger capability rather than against
/// any principal credential path.
pub fn is_task_trigger_presentation(presented: &str) -> bool {
    presented.starts_with(TASK_TRIGGER_PREFIX)
}

/// Constant-time verifier comparison. Length mismatch fails without
/// early content disclosure beyond length.
pub fn verify_trigger_secret(presented_secret: &str, stored_verifier_hex: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    let presented_digest = task_trigger_digest_hex(presented_secret);
    let left = presented_digest.as_bytes();
    let right = stored_verifier_hex.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).unwrap_u8() == 1
}

/// Bounded non-secret placeholder for logs and diagnostics. Preserves
/// only the bearer kind; never echoes any credential segment.
pub fn redact_presented_trigger(presented: &str) -> &'static str {
    let _ = presented;
    "[REDACTED:task-trigger]"
}

// ── Validation ────────────────────────────────────────────────────────

/// Validate an optional absolute expiry timestamp (milliseconds).
/// Expiry must lie strictly in the future relative to `now_ms`.
pub fn validate_trigger_expires_at(
    value: Option<i64>,
    now_ms: i64,
) -> Result<Option<i64>, crate::work_order::WorkOrderError> {
    use crate::work_order::WorkOrderError;
    let Some(at) = value else {
        return Ok(None);
    };
    if at <= now_ms {
        return Err(WorkOrderError::invalid(
            "expires_at_ms",
            "trigger expiry must lie in the future",
        ));
    }
    if at > crate::work_order::MAX_NOT_BEFORE_MS {
        return Err(WorkOrderError::invalid(
            "expires_at_ms",
            "trigger expiry is outside supported bounds",
        ));
    }
    Ok(Some(at))
}

/// Validate an optional maximum fire count. `None` means unbounded
/// within the trigger's active lifetime (still one latch per
/// occurrence); `Some(n)` requires `n >= 1`.
pub fn validate_trigger_max_fires(
    value: Option<u32>,
) -> Result<Option<u32>, crate::work_order::WorkOrderError> {
    use crate::work_order::WorkOrderError;
    let Some(max) = value else {
        return Ok(None);
    };
    if max == 0 || max > MAX_TRIGGER_MAX_FIRES {
        return Err(WorkOrderError::invalid(
            "max_fires",
            format!("max fires must be 1..={MAX_TRIGGER_MAX_FIRES}"),
        ));
    }
    Ok(Some(max))
}

/// Validate a caller-supplied fire idempotency key.
pub fn validate_trigger_idempotency_key(
    value: Option<&str>,
) -> Result<Option<String>, crate::work_order::WorkOrderError> {
    use crate::work_order::WorkOrderError;
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_TRIGGER_IDEMPOTENCY_KEY_LEN {
        return Err(WorkOrderError::invalid(
            "idempotency_key",
            format!("idempotency key must be 1..={MAX_TRIGGER_IDEMPOTENCY_KEY_LEN} bytes"),
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(WorkOrderError::invalid(
            "idempotency_key",
            "idempotency key contains an unsupported character",
        ));
    }
    Ok(Some(trimmed.to_owned()))
}

pub fn validate_trigger_list_limit(value: Option<u32>) -> u32 {
    value
        .unwrap_or(DEFAULT_TRIGGER_LIST_LIMIT)
        .clamp(1, MAX_TRIGGER_LIST_LIMIT)
}

/// Structural audit metadata for one trigger management mutation.
/// Locators, revisions, and counts only: never secrets or verifiers.
pub fn audit_metadata_for_task_trigger(
    trigger: &TaskTrigger,
    operation: &str,
    decision_id: &str,
) -> std::collections::BTreeMap<String, String> {
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert(
        "project.id".to_owned(),
        trigger.project_id.as_str().to_owned(),
    );
    metadata.insert("task_trigger.id".to_owned(), trigger.id.as_str().to_owned());
    metadata.insert(
        "work_order.id".to_owned(),
        trigger.work_order_id.as_str().to_owned(),
    );
    metadata.insert("task_trigger.op".to_owned(), operation.to_owned());
    metadata.insert(
        "task_trigger.state".to_owned(),
        trigger.state.as_str().to_owned(),
    );
    metadata.insert(
        "task_trigger.fire_count".to_owned(),
        trigger.fire_count.to_string(),
    );
    metadata.insert("decision.id".to_owned(), decision_id.to_owned());
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_bearers_have_required_shape_and_entropy() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..32 {
            let (id, plaintext, verifier) = generate_task_trigger();
            assert!(plaintext.starts_with(TASK_TRIGGER_PREFIX));
            let (locator, secret) = split_presented_trigger(&plaintext).expect("parseable");
            assert_eq!(locator, id.as_str());
            // 32 bytes -> 43 base64url-no-pad characters.
            assert_eq!(secret.len(), 43);
            assert!(verify_trigger_secret(&secret, &verifier));
            assert!(!plaintext.contains(&verifier));
            assert!(seen.insert(plaintext));
        }
    }

    #[test]
    fn verifier_never_round_trips_to_secret() {
        let (_, plaintext, verifier) = generate_task_trigger();
        let (_, secret) = split_presented_trigger(&plaintext).expect("parseable");
        // A verifier alone cannot authenticate: verification needs the secret.
        assert!(!verify_trigger_secret(&verifier, &verifier));
        assert!(verify_trigger_secret(&secret, &verifier));
        // One flipped character fails.
        let mut wrong = secret.clone();
        let first = if wrong.starts_with('A') { 'B' } else { 'A' };
        wrong.replace_range(0..1, &first.to_string());
        assert!(!verify_trigger_secret(&wrong, &verifier));
    }

    #[test]
    fn malformed_presentations_fail_closed() {
        assert!(split_presented_trigger("").is_none());
        assert!(split_presented_trigger("cggt_some-id.secret").is_none());
        assert!(split_presented_trigger("cggtr_").is_none());
        assert!(split_presented_trigger("cggtr_.secret").is_none());
        assert!(split_presented_trigger("cggtr_id.").is_none());
        assert!(split_presented_trigger("cggtr_id with space.secret").is_none());
        assert!(split_presented_trigger("cggtr_../escape.secret").is_none());
        assert!(split_presented_trigger(&format!("cggtr_id.{}", "s".repeat(257))).is_none());
        assert!(split_presented_trigger(&"x".repeat(MAX_PRESENTED_TRIGGER_LEN + 1)).is_none());
    }

    #[test]
    fn presentation_routing_never_collides_with_personal_tokens() {
        let (_, plaintext, _) = generate_task_trigger();
        assert!(is_task_trigger_presentation(&plaintext));
        assert!(!crate::transport_auth::is_personal_token_presentation(
            &plaintext
        ));
        assert!(!is_task_trigger_presentation("cggt_token-id.secret"));
    }

    #[test]
    fn redaction_never_echoes_credential_segments() {
        let (_, plaintext, _) = generate_task_trigger();
        let redacted = redact_presented_trigger(&plaintext);
        assert!(!redacted.contains(&plaintext));
        for segment in plaintext.split('.') {
            assert!(!redacted.contains(segment) || segment == "cggtr_");
        }
    }

    #[test]
    fn trigger_debug_omits_verifier() {
        let (id, _, verifier) = generate_task_trigger();
        let trigger = TaskTrigger {
            id,
            revision: 1,
            project_id: ProjectId::parse("project-1").unwrap(),
            work_order_id: WorkOrderId::parse("wo-1").unwrap(),
            trigger_ref: "hook-1".to_owned(),
            secret_verifier_hex: verifier.clone(),
            verifier_version: TASK_TRIGGER_VERIFIER_VERSION.to_owned(),
            state: TaskTriggerState::Active,
            created_by: PrincipalId::parse("local-owner").unwrap(),
            created_at_ms: 1,
            expires_at_ms: None,
            max_fires: None,
            fire_count: 0,
            last_fired_at_ms: None,
        };
        let debug = format!("{trigger:?}");
        assert!(!debug.contains(&verifier));
        assert!(debug.contains("task-trigger") || debug.contains("TaskTrigger"));
    }

    #[test]
    fn status_derives_expiry_and_exhaustion() {
        let (id, _, verifier) = generate_task_trigger();
        let mut trigger = TaskTrigger {
            id,
            revision: 1,
            project_id: ProjectId::parse("project-1").unwrap(),
            work_order_id: WorkOrderId::parse("wo-1").unwrap(),
            trigger_ref: "hook-1".to_owned(),
            secret_verifier_hex: verifier,
            verifier_version: TASK_TRIGGER_VERIFIER_VERSION.to_owned(),
            state: TaskTriggerState::Active,
            created_by: PrincipalId::parse("local-owner").unwrap(),
            created_at_ms: 1_000,
            expires_at_ms: Some(2_000),
            max_fires: Some(1),
            fire_count: 0,
            last_fired_at_ms: None,
        };
        assert_eq!(trigger.status_at(1_500), TaskTriggerStatus::Active);
        assert_eq!(trigger.status_at(2_000), TaskTriggerStatus::Expired);
        trigger.expires_at_ms = None;
        trigger.fire_count = 1;
        assert_eq!(trigger.status_at(1_500), TaskTriggerStatus::Exhausted);
        trigger.state = TaskTriggerState::Revoked;
        trigger.fire_count = 0;
        assert_eq!(trigger.status_at(1_500), TaskTriggerStatus::Revoked);
    }

    #[test]
    fn fire_outcome_status_vocabulary_is_narrow() {
        let accepted = FireOutcome {
            latched: true,
            receipt_id: "receipt-1".to_owned(),
            duplicate: false,
        };
        let replay = FireOutcome {
            latched: false,
            receipt_id: "receipt-1".to_owned(),
            duplicate: true,
        };
        assert_eq!(accepted.status(), "accepted");
        assert_eq!(replay.status(), "already_fired");
        let json = serde_json::to_string(&replay).expect("serialize");
        assert!(!json.contains("secret"));
        assert!(!json.contains("project"));
    }
}
