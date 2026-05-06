use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};

use crate::bus::QuestionRegistry;
use crate::error::{AppError, StorageError};

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
) -> Result<Json<QuestionResponse>, AppError> {
    if req.session_id != session_id {
        return Err(AppError::Storage(StorageError::NotFound(
            "session id mismatch".to_string(),
        )));
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
        )));
    }

    Ok(Json(QuestionResponse {
        session_id,
        status: "answered".to_string(),
    }))
}

pub async fn get_pending_questions(
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(get_pending_questions_for_session(&session_id)))
}

/// Helper function that returns pending questions for a session.
/// This can be called directly in tests without Axum extractors.
pub fn get_pending_questions_for_session(session_id: &str) -> serde_json::Value {
    let pending_ids = crate::bus::QuestionRegistry::pending_question_ids();

    // Filter to only include the requested session_id if it has pending questions
    let questions: Vec<serde_json::Value> = pending_ids
        .iter()
        .filter(|id| *id == session_id)
        .map(|id| {
            serde_json::json!({
                "question_id": id,
                "session_id": id,
            })
        })
        .collect();

    serde_json::json!({
        "questions": questions
    })
}
