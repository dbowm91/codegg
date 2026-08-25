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
        return Err(
            AppError::Storage(StorageError::NotFound("session id mismatch".to_string())).into(),
        );
    }

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
    // never matched a real registration.
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
