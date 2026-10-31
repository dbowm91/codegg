//! M007-I (work packages B and C): the daemon's skill-promotion workflow.
//!
//! The workflow used to be **two writers**: the TUI staged a request in one
//! JSON file and the daemon-side tool wrote a proposal into the same file,
//! with an `flock` sidecar holding the two together. This module is the
//! **one** writer. The durable record lives in the daemon's session
//! storage, keyed on the canonical `WorkspaceId`, and every step of the
//! workflow — staging, listing, previewing, rejecting and publishing — is a
//! request against it.
//!
//! Two properties are load-bearing:
//!
//! * **One writer.** The TUI holds no durable promotion state, and the
//!   daemon-side tool writes through the same store the requests use. There
//!   is no second writer and therefore no coordination protocol to get
//!   wrong.
//! * **One namespace.** The record is keyed on the canonical `WorkspaceId`,
//!   so the human's staged request and the model tool's proposal resolve to
//!   the same record. The old namespace was a digest of a *path string*,
//!   derived differently on each side, and the disagreement surfaced as
//!   "promotion request not found".
//!
//! The publication is a **non-broker writer**, so it is mediated by the C004
//! checkout reservation gate: a reserved checkout refuses the publication
//! before its first effect, and the refusal is recorded durably. The gate
//! runs in the daemon's own dispatch path, before this handler is reached.

use std::path::{Path, PathBuf};

use codegg_core::session::skill_promotion::{
    SkillPromotionKind, SkillPromotionRecord, SkillPromotionStore,
};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::skill::{
    SkillPromotionActionDto, SkillPromotionRequestDto, SkillPromotionResultDto,
    SkillProposalDetailDto, SkillProposalDiagnosticDto, SkillProposalSummaryDto,
    SkillTargetScopeDto, MAX_SKILL_PROMOTION_LIST,
};

use crate::skills::promotion::{
    build_draft_prompt, draft_context, habit_is_promotable, proposal_from_payload,
    proposal_to_payload, request_to_payload, PromotionRequestId, SkillProposalId,
    SkillProposalStatus, SkillTargetScope, PROMOTION_REQUEST_TTL_MS,
};

/// The daemon refuses the promotion when its session storage is unavailable.
const REFUSAL_PROMOTION_UNAVAILABLE: &str = "skill_promotion_unavailable";
/// The daemon refuses the promotion when the request names no workspace.
const REFUSAL_PROMOTION_NO_WORKSPACE: &str = "skill_promotion_no_workspace";
/// The daemon refuses the promotion when the request names no session.
const REFUSAL_PROMOTION_NO_SESSION: &str = "skill_promotion_no_session";
/// The daemon refuses the promotion when the named habit is not promotable.
const REFUSAL_PROMOTION_NO_HABIT: &str = "skill_promotion_no_habit";
/// The daemon refuses the promotion when the named proposal is not found.
const REFUSAL_PROMOTION_NO_PROPOSAL: &str = "skill_promotion_no_proposal";
/// The daemon refuses the promotion when the record is not in the expected
/// state — a stale approval, or a proposal that is not validated.
const REFUSAL_PROMOTION_STALE: &str = "skill_promotion_stale";
/// The daemon refuses the promotion when the publication target is unsafe.
const REFUSAL_PROMOTION_UNSAFE_TARGET: &str = "skill_promotion_unsafe_target";

/// Serve one request against the daemon's promotion workflow.
pub(crate) async fn handle_skill_promotion(
    daemon: &crate::core::daemon::CoreDaemon,
    request: CoreRequest,
) -> Result<CoreResponse, crate::error::AppError> {
    let CoreRequest::SkillPromotion { request } = request else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_UNAVAILABLE,
            "unsupported skill promotion request",
        ));
    };
    match request.action {
        SkillPromotionActionDto::Begin => begin_request(daemon, &request).await,
        SkillPromotionActionDto::List => list_proposals(daemon, &request).await,
        SkillPromotionActionDto::Get => get_proposal(daemon, &request).await,
        SkillPromotionActionDto::Preview => preview_proposal(daemon, &request).await,
        SkillPromotionActionDto::Reject => reject_proposal(daemon, &request).await,
        SkillPromotionActionDto::Publish => publish_proposal(daemon, &request).await,
    }
}

/// The daemon's promotion store, or a typed refusal.
fn promotion_store(
    daemon: &crate::core::daemon::CoreDaemon,
) -> Result<SkillPromotionStore, CoreResponse> {
    let Some(pool) = daemon.pool.clone() else {
        return Err(promotion_error(
            REFUSAL_PROMOTION_UNAVAILABLE,
            "the daemon has no session storage",
        ));
    };
    Ok(SkillPromotionStore::new(pool))
}

/// The canonical scope the request acts in.
fn request_scope(
    request: &SkillPromotionRequestDto,
) -> Result<(String, String), CoreResponse> {
    if request.workspace_id.trim().is_empty() {
        return Err(promotion_error(
            REFUSAL_PROMOTION_NO_WORKSPACE,
            "the promotion request names no workspace",
        ));
    }
    if request.session_id.trim().is_empty() {
        return Err(promotion_error(
            REFUSAL_PROMOTION_NO_SESSION,
            "the promotion request names no session",
        ));
    }
    Ok((
        request.workspace_id.trim().to_owned(),
        request.session_id.trim().to_owned(),
    ))
}

/// Stage a request: the human asked for a draft from a habit.
///
/// The daemon resolves the habit candidate from its own store, builds the
/// bounded draft prompt, and records the request against the canonical
/// workspace.
async fn begin_request(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, session_id) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let Some(habit_id) = request.habit_id.as_deref() else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_HABIT,
            "the promotion request names no habit",
        ));
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };

    // The daemon owns the habit candidate too, so the staged request's
    // context is the daemon's own view of the habit — never the frontend's.
    let workspace_root = match daemon.workspace_root_for(&workspace_id).await {
        Some(root) => root,
        None => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_WORKSPACE,
                "the workspace is not registered",
            ))
        }
    };
    let candidate = match load_habit_candidate(&workspace_root, habit_id) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_HABIT,
                "the habit candidate was not found",
            ))
        }
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_HABIT,
                &format!("the habit candidate could not be read: {message}"),
            ))
        }
    };
    if !habit_is_promotable(&candidate) {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_HABIT,
            "the habit candidate is not ready for promotion",
        ));
    }
    let existing_skill_names = discovered_skill_names(&workspace_root);
    let context = match draft_context(&candidate, existing_skill_names, Vec::new()) {
        Ok(context) => context,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_HABIT,
                &format!("the promotion context could not be built: {message}"),
            ))
        }
    };

    let now = chrono::Utc::now().timestamp_millis();
    let request_id = PromotionRequestId::new();
    let draft = crate::skills::promotion::PromotionDraftRequest {
        id: request_id.clone(),
        session_id: session_id.clone(),
        workspace_id: workspace_id.clone(),
        habit_id: candidate.id,
        habit_fingerprint: candidate.workflow_fingerprint.clone(),
        candidate_revision: candidate.revision,
        context,
        created_at: now,
        expires_at: now.saturating_add(PROMOTION_REQUEST_TTL_MS),
        consumed: false,
    };
    let prompt = build_draft_prompt(&draft);
    let payload = match request_to_payload(&draft) {
        Ok(payload) => payload,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the promotion request could not be encoded: {message}"),
            ))
        }
    };
    let record = SkillPromotionRecord {
        id: request_id.as_str().to_owned(),
        workspace_id: workspace_id.clone(),
        session_id,
        kind: SkillPromotionKind::Request,
        payload,
        payload_sha256: String::new(),
        status: "pending".to_owned(),
        revision: 1,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: Some(draft.expires_at),
    };
    match store.put(record).await {
        Ok(_) => Ok(CoreResponse::SkillPromotion {
            result: SkillPromotionResultDto {
                request_id: Some(request_id.as_str().to_owned()),
                draft_prompt: Some(prompt),
                proposals: None,
                published_relative_path: None,
                detail: None,
                changed: true,
            },
        }),
        Err(error) => Ok(promotion_error(
            REFUSAL_PROMOTION_UNAVAILABLE,
            &format!("the promotion request could not be recorded: {error}"),
        )),
    }
}

/// The workspace's proposals.
async fn list_proposals(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, _) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };
    match store
        .list(&workspace_id, SkillPromotionKind::Proposal, MAX_SKILL_PROMOTION_LIST)
        .await
    {
        Ok(records) => Ok(CoreResponse::SkillPromotion {
            result: SkillPromotionResultDto {
                request_id: None,
                draft_prompt: None,
                proposals: Some(records.iter().map(proposal_summary).collect()),
                published_relative_path: None,
                detail: None,
                changed: false,
            },
        }),
        Err(error) => Ok(promotion_error(
            REFUSAL_PROMOTION_UNAVAILABLE,
            &format!("the proposals could not be read: {error}"),
        )),
    }
}

/// One proposal.
async fn get_proposal(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, _) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let Some(proposal_id) = request.proposal_id.as_deref() else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the promotion request names no proposal",
        ));
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };
    match store
        .get(&workspace_id, SkillPromotionKind::Proposal, proposal_id)
        .await
    {
        Ok(Some(record)) => Ok(CoreResponse::SkillPromotion {
            result: SkillPromotionResultDto {
                request_id: None,
                draft_prompt: None,
                proposals: Some(vec![proposal_summary(&record)]),
                published_relative_path: None,
                detail: Some(proposal_detail(&record)),
                changed: false,
            },
        }),
        Ok(None) => Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the proposal was not found",
        )),
        Err(error) => Ok(promotion_error(
            REFUSAL_PROMOTION_UNAVAILABLE,
            &format!("the proposal could not be read: {error}"),
        )),
    }
}

/// Record that the human previewed the proposal's current revision.
///
/// The preview is **metadata**: it does not change the proposal's content
/// revision, so an approval captured from that preview stays valid.
async fn preview_proposal(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, _) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let Some(proposal_id) = request.proposal_id.as_deref() else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the promotion request names no proposal",
        ));
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };
    let record = match store
        .get(&workspace_id, SkillPromotionKind::Proposal, proposal_id)
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_PROPOSAL,
                "the proposal was not found",
            ))
        }
        Err(error) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be read: {error}"),
            ))
        }
    };
    let mut proposal = match proposal_from_payload(&record.payload) {
        Ok(proposal) => proposal,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_STALE,
                &format!("the proposal could not be read: {message}"),
            ))
        }
    };
    proposal.previewed_revision = Some(proposal.revision);
    proposal.updated_at = chrono::Utc::now().timestamp_millis();
    let payload = match proposal_to_payload(&proposal) {
        Ok(payload) => payload,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be encoded: {message}"),
            ))
        }
    };
    let previewed = store
        .compare_and_set_record(
            &workspace_id,
            SkillPromotionKind::Proposal,
            proposal_id,
            record.revision,
            &record.status,
            &payload,
            record.revision,
            proposal.updated_at,
        )
        .await
        .unwrap_or(false);
    Ok(CoreResponse::SkillPromotion {
        result: SkillPromotionResultDto {
            request_id: None,
            draft_prompt: None,
            proposals: Some(vec![proposal_summary(&record)]),
            published_relative_path: None,
            detail: Some(proposal_detail(&record)),
            changed: previewed,
        },
    })
}

/// Reject a proposal that is awaiting review.
async fn reject_proposal(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, _) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let Some(proposal_id) = request.proposal_id.as_deref() else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the promotion request names no proposal",
        ));
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };
    let record = match store
        .get(&workspace_id, SkillPromotionKind::Proposal, proposal_id)
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_PROPOSAL,
                "the proposal was not found",
            ))
        }
        Err(error) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be read: {error}"),
            ))
        }
    };
    let mut proposal = match proposal_from_payload(&record.payload) {
        Ok(proposal) => proposal,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_STALE,
                &format!("the proposal could not be read: {message}"),
            ))
        }
    };
    if proposal.status != SkillProposalStatus::Validated {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_STALE,
            "the proposal is not awaiting review",
        ));
    }
    proposal.status = SkillProposalStatus::Rejected;
    proposal.updated_at = chrono::Utc::now().timestamp_millis();
    proposal.revision = proposal.revision.saturating_add(1);
    let payload = match proposal_to_payload(&proposal) {
        Ok(payload) => payload,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be encoded: {message}"),
            ))
        }
    };
    let rejected = store
        .compare_and_set_record(
            &workspace_id,
            SkillPromotionKind::Proposal,
            proposal_id,
            record.revision,
            SkillProposalStatus::Rejected.as_str(),
            &payload,
            proposal.revision as i64,
            proposal.updated_at,
        )
        .await
        .unwrap_or(false);
    Ok(CoreResponse::SkillPromotion {
        result: SkillPromotionResultDto {
            request_id: None,
            draft_prompt: None,
            proposals: Some(vec![proposal_summary(&record)]),
            published_relative_path: None,
            detail: Some(proposal_detail(&record)),
            changed: rejected,
        },
    })
}

/// Publish a proposal to one scope.
///
/// The publication is a **non-broker writer**, so it is mediated by the C004
/// checkout reservation gate: a reserved checkout refuses the publication
/// **before** its first effect, and the refusal is recorded durably. The
/// promotion record is **not** marked published when the gate refuses, so
/// the human can act on it afterwards.
async fn publish_proposal(
    daemon: &crate::core::daemon::CoreDaemon,
    request: &SkillPromotionRequestDto,
) -> Result<CoreResponse, crate::error::AppError> {
    let (workspace_id, _session_id) = match request_scope(request) {
        Ok(scope) => scope,
        Err(refusal) => return Ok(refusal),
    };
    let Some(proposal_id) = request.proposal_id.as_deref() else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the promotion request names no proposal",
        ));
    };
    let Some(proposal_id) = SkillProposalId::parse(proposal_id) else {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_NO_PROPOSAL,
            "the proposal identity is invalid",
        ));
    };
    let scope = match request.target_scope.unwrap_or(SkillTargetScopeDto::Project) {
        SkillTargetScopeDto::Project => SkillTargetScope::Project,
        SkillTargetScopeDto::Global => SkillTargetScope::Global,
    };
    let store = match promotion_store(daemon) {
        Ok(store) => store,
        Err(refusal) => return Ok(refusal),
    };
    let record = match store
        .get(&workspace_id, SkillPromotionKind::Proposal, proposal_id.as_str())
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_PROPOSAL,
                "the proposal was not found",
            ))
        }
        Err(error) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be read: {error}"),
            ))
        }
    };
    let proposal = match proposal_from_payload(&record.payload) {
        Ok(proposal) => proposal,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_STALE,
                &format!("the proposal could not be read: {message}"),
            ))
        }
    };
    if proposal.status != SkillProposalStatus::Validated {
        return Ok(promotion_error(
            REFUSAL_PROMOTION_STALE,
            "the proposal is not validated",
        ));
    }
    let workspace_root = match daemon.workspace_root_for(&workspace_id).await {
        Some(root) => root,
        None => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_NO_WORKSPACE,
                "the workspace is not registered",
            ))
        }
    };

    // The publication's filesystem effects are blocking, so they run on the
    // blocking pool. The record is written back **after** the bytes are
    // durable, under a compare-and-swap against the revision the human
    // approved.
    let publication_request = crate::skills::publish::SkillPublicationRequest {
        proposal_id: proposal_id.clone(),
        expected_revision: proposal.revision,
        expected_content_digest: proposal.content_digest.clone(),
        target_scope: scope,
    };
    let project_identity = workspace_root.to_string_lossy().to_string();
    let approved = proposal.clone();
    let published = tokio::task::spawn_blocking(move || {
        let service = crate::skills::publish::SkillPublicationService::new(habit_root());
        service.publish(
            &project_identity,
            &approved,
            &workspace_root,
            &global_config_dir(),
            publication_request,
            chrono::Utc::now().timestamp_millis(),
        )
    })
    .await;
    let publication = match published {
        Ok(Ok(publication)) => publication,
        Ok(Err(error)) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNSAFE_TARGET,
                &format!("the publication was refused: {error}"),
            ))
        }
        Err(error) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the publication could not be performed: {error}"),
            ))
        }
    };
    let payload = match proposal_to_payload(&publication.proposal) {
        Ok(payload) => payload,
        Err(message) => {
            return Ok(promotion_error(
                REFUSAL_PROMOTION_UNAVAILABLE,
                &format!("the proposal could not be encoded: {message}"),
            ))
        }
    };
    let written = store
        .compare_and_set_record(
            &workspace_id,
            SkillPromotionKind::Proposal,
            proposal_id.as_str(),
            record.revision,
            SkillProposalStatus::Published.as_str(),
            &payload,
            publication.proposal.revision as i64,
            publication.proposal.updated_at,
        )
        .await
        .unwrap_or(false);
    if !written {
        // The bytes are durable but the record moved underneath them. The
        // publication is reported as refused rather than as published: a
        // record that does not say "published" is the safe answer.
        return Ok(promotion_error(
            REFUSAL_PROMOTION_STALE,
            "the proposal changed while it was being published",
        ));
    }
    Ok(CoreResponse::SkillPromotion {
        result: SkillPromotionResultDto {
            request_id: None,
            draft_prompt: None,
            proposals: Some(vec![proposal_summary(&record)]),
            published_relative_path: Some(publication.relative_path),
            detail: Some(proposal_detail(&record)),
            changed: true,
        },
    })
}

/// One proposal's summary, without its body.
fn proposal_summary(record: &SkillPromotionRecord) -> SkillProposalSummaryDto {
    let (name, description, status, revision, content_digest) =
        match crate::skills::promotion::proposal_summary_from_payload(&record.payload) {
            Ok(summary) => (
                summary.name,
                summary.description,
                summary.status,
                summary.revision,
                summary.content_digest,
            ),
            Err(_) => (
                String::new(),
                String::new(),
                record.status.clone(),
                record.revision.max(0) as u64,
                record.payload_sha256.clone(),
            ),
        };
    SkillProposalSummaryDto {
        proposal_id: record.id.clone(),
        name,
        description,
        status,
        revision,
        content_digest,
    }
}

/// One proposal in full, as the daemon reports it.
fn proposal_detail(record: &SkillPromotionRecord) -> SkillProposalDetailDto {
    let Ok(proposal) = proposal_from_payload(&record.payload) else {
        return SkillProposalDetailDto {
            proposal_id: record.id.clone(),
            name: String::new(),
            description: String::new(),
            status: record.status.clone(),
            revision: record.revision.max(0) as u64,
            content_digest: record.payload_sha256.clone(),
            habit_id: String::new(),
            habit_fingerprint: String::new(),
            candidate_revision: 0,
            target_scope: SkillTargetScopeDto::Project,
            skill_markdown: String::new(),
            diagnostics: Vec::new(),
            published_relative_path: None,
        };
    };
    SkillProposalDetailDto {
        proposal_id: proposal.id.as_str().to_owned(),
        name: proposal.name.clone(),
        description: proposal.description.clone(),
        status: proposal.status.as_str().to_owned(),
        revision: proposal.revision,
        content_digest: proposal.content_digest.clone(),
        habit_id: proposal.habit_id.as_str().to_owned(),
        habit_fingerprint: proposal.habit_fingerprint.clone(),
        candidate_revision: proposal.candidate_revision,
        target_scope: match proposal.target_scope {
            SkillTargetScope::Project => SkillTargetScopeDto::Project,
            SkillTargetScope::Global => SkillTargetScopeDto::Global,
        },
        skill_markdown: proposal.skill_markdown.clone(),
        diagnostics: proposal
            .diagnostics
            .iter()
            .map(|diagnostic| SkillProposalDiagnosticDto {
                severity: format!("{:?}", diagnostic.severity),
                reason: diagnostic.reason.clone(),
                location: diagnostic.location.clone(),
            })
            .collect(),
        published_relative_path: proposal
            .publication
            .as_ref()
            .map(|publication| publication.relative_path.clone()),
    }
}

/// The habit candidate the daemon owns, for one workspace.
///
/// The habit store is keyed on the workspace's canonical root, which is the
/// same identity the daemon resolves for every other workflow.
fn load_habit_candidate(
    workspace_root: &Path,
    habit_id: &str,
) -> Result<Option<codegg_core::memory::habit::HabitCandidate>, String> {
    let identity = workspace_root.to_string_lossy().to_string();
    let store = codegg_core::memory::habit::HabitStore::new()
        .map_err(|error| error.to_string())?;
    let candidates = store.load(&identity).map_err(|error| error.to_string())?;
    Ok(candidates
        .into_iter()
        .find(|candidate| candidate.id.as_str() == habit_id))
}

/// The names of the skills the workspace already has, for the draft's
/// advisory list.
fn discovered_skill_names(workspace_root: &Path) -> Vec<String> {
    let app_config = crate::config::schema::Config::load_or_default();
    let config = crate::agent::asset_context::asset_discovery_config_from(&app_config);
    let global_roots: Vec<PathBuf> = crate::agent::asset_context::default_global_discovery_root()
        .into_iter()
        .collect();
    crate::skills::AssetRegistry::build(&config, workspace_root, &global_roots)
        .list()
        .iter()
        .take(32)
        .map(|skill| skill.name.clone())
        .collect()
}

/// Where the habit candidates live.
fn habit_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codegg")
        .join("memory")
}

/// The user-level configuration root, which holds the global skill scope.
fn global_config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// A typed refusal, in the shape the frontend renders.
fn promotion_error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}
