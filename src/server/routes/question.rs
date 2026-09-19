use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::authz;
use super::super::state::ServerState;
use crate::bus::QuestionRegistry;
use crate::error::{AppError, AxumAppError, StorageError};
use codegg_core::transport_auth::AuthenticatedPrincipal;

#[derive(Deserialize)]
pub struct SubmitQuestionRequest {
    pub session_id: String,
    pub answers: serde_json::Value,
}

#[derive(Serialize)]
pub struct QuestionResponse {
    pub session_id: String,
    pub status: String,
}

pub async fn submit_question(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
    Json(req): Json<SubmitQuestionRequest>,
) -> Result<Json<QuestionResponse>, AxumAppError> {
    if req.session_id != session_id {
        return Err(authz::denial_not_found());
    }

    // Resolve the pending items to the canonical owning session before
    // mutating. Unknown sessions fail closed with the privacy-safe shape
    // so cross-project IDs never leak.
    let pending = QuestionRegistry::get_pending_for_session(&session_id);
    if pending.is_empty() {
        return Err(authz::denial_not_found());
    }
    // M004: all pending answers must belong to one turn; ambiguous
    // turns fail closed before the controller check.
    let mut turns: Vec<String> = pending
        .iter()
        .filter_map(|item| item.turn_id.clone())
        .collect();
    turns.sort();
    turns.dedup();
    if turns.len() != 1 {
        return Err(authz::denial_not_found());
    }
    authz::authorize_control_response_for_turn(
        &state.pool,
        &principal,
        &session_id,
        Some(&turns[0]),
        "question_respond",
    )
    .await?;

    // Normalize answers to consistent JSON string format
    // Accepts both Vec<String> and object mapping question IDs to answers
    let answers_json = serde_json::to_string(&req.answers).map_err(|e| {
        AppError::Storage(StorageError::Database(format!(
            "failed to serialize answers: {}",
            e
        )))
    })?;

    // Questions are keyed by their registry id (`q-{uuid}`) and owned by
    // a session. Answer every pending question owned by this session —
    // the legacy path looked up `session_id` as a question key, which
    // never matched a real registration. Re-read after the
    // authorization gate so a concurrent answer cannot double-respond.
    let pending = QuestionRegistry::get_pending_for_session(&session_id);
    let mut answered_any = false;
    for info in pending {
        if QuestionRegistry::answer_question_scoped(
            &session_id,
            &info.question_id,
            answers_json.clone(),
        ) {
            answered_any = true;
        }
    }

    if !answered_any {
        return Err(authz::denial_not_found());
    }

    Ok(Json(QuestionResponse {
        session_id,
        status: "answered".to_string(),
    }))
}

pub async fn get_pending_questions(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AxumAppError> {
    authz::authorize_session(
        &state.pool,
        &principal,
        &session_id,
        codegg_core::authorization::Capability::SessionRead,
        "question_list",
    )
    .await?;
    Ok(Json(get_pending_questions_for_session(&session_id)))
}

/// Helper function that returns pending questions owned by `session_id`.
/// This can be called directly in tests without Axum extractors.
pub fn get_pending_questions_for_session(session_id: &str) -> serde_json::Value {
    let questions: Vec<serde_json::Value> = QuestionRegistry::get_pending_for_session(session_id)
        .into_iter()
        .map(|q| {
            serde_json::json!({
                "question_id": q.question_id,
                "session_id": q.session_id,
                "turn_id": q.turn_id,
                "age_ms": q.created_at.elapsed().as_millis() as u64,
            })
        })
        .collect();

    serde_json::json!({
        "questions": questions
    })
}
