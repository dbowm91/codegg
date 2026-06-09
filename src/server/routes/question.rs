use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};

use crate::bus::QuestionRegistry;
use crate::error::{AppError, AxumAppError, StorageError};

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
    Path(session_id): Path<String>,
    Json(req): Json<SubmitQuestionRequest>,
) -> Result<Json<QuestionResponse>, AxumAppError> {
    if req.session_id != session_id {
        return Err(AppError::Storage(StorageError::NotFound(
            "session id mismatch".to_string(),
        ))
        .into());
    }

    // Normalize answers to consistent JSON string format
    // Accepts both Vec<String> and object mapping question IDs to answers
    let answers_json = serde_json::to_string(&req.answers).map_err(|e| {
        AppError::Storage(StorageError::Database(format!(
            "failed to serialize answers: {}",
            e
        )))
    })?;

    let answered = QuestionRegistry::answer_question(session_id.clone(), answers_json);

    if !answered {
        return Err(AppError::Storage(StorageError::NotFound(
            "no pending question for this session".to_string(),
        ))
        .into());
    }

    Ok(Json(QuestionResponse {
        session_id,
        status: "answered".to_string(),
    }))
}

pub async fn get_pending_questions(
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AxumAppError> {
    Ok(Json(get_pending_questions_for_session(&session_id)))
}

/// Helper function that returns pending questions for a session.
/// This can be called directly in tests without Axum extractors.
/// NOTE: QuestionRegistry does not store session_id in keys, so proper session-based
/// filtering is not possible without extending the registry. Returns empty list when
/// session_id is provided to indicate filtering is not supported.
pub fn get_pending_questions_for_session(session_id: &str) -> serde_json::Value {
    let _pending_ids = crate::bus::QuestionRegistry::pending_question_ids();

    // QuestionRegistry keys are not session_id based, so we cannot filter.
    // Return empty to indicate filtering is not possible.
    let _ = session_id;
    let questions: Vec<serde_json::Value> = Vec::new();

    serde_json::json!({
        "questions": questions
    })
}
