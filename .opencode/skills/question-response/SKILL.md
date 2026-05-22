---
name: question-response
description: Question and permission response shape consistency across HTTP, WebSocket, and local registry
tags: [question, permission, http, websocket, registry]
---

# Question/Permission Response Shape Guide

This skill covers the consistent handling of question answers and permission responses across different paths (HTTP routes, WebSocket messages, and local registry).

## Question Answer Format

### Answer Formats Accepted

Both `Vec<String>` and object mapping question IDs to answers are accepted:

```rust
// Format 1: Vec<String> (simple, ordered answers)
let answers = serde_json::json!(["answer1", "answer2"]);

// Format 2: Object mapping IDs to answers (structured)
let answers = serde_json::json!({
    "q1": "blue",
    "q2": "red"
});
```

### HTTP Route (`src/server/routes/question.rs`)

```rust
#[derive(Deserialize)]
pub struct SubmitQuestionRequest {
    pub session_id: String,
    pub answers: serde_json::Value,  // Flexible: accepts both formats
}

pub async fn submit_question(
    Path(session_id): Path<String>,
    Json(req): Json<SubmitQuestionRequest>,
) -> Result<Json<QuestionResponse>, AppError> {
    // Normalize answers to consistent JSON string
    let answers_json = serde_json::to_string(&req.answers).map_err(|e| {
        AppError::Storage(StorageError::Database(format!("failed to serialize: {}", e)))
    })?;
    
    let answered = QuestionRegistry::answer_question(session_id, answers_json);
    // ...
}
```

### WebSocket Message (`src/server/ws.rs`)

```rust
enum TuiMessage {
    QuestionResponse { id: String, answers: serde_json::Value },  // Updated in Packet 4
    // ...
}

// Handling:
TuiMessage::QuestionResponse { id, answers } => {
    let answers_json = match serde_json::to_string(&answers) {
        Ok(json) => json,
        Err(_) => return,
    };
    let _ = crate::bus::QuestionRegistry::answer_question(id, answers_json);
}
```

### Local Registry (`src/bus/mod.rs`)

```rust
impl QuestionRegistry {
    pub fn answer_question(question_id: String, answers: String) -> bool {
        // answers is a JSON string (either format)
        // The agent loop receives this string via oneshot channel
    }
}
```

### Question Tool (`src/tool/question.rs`)

```rust
pub fn format_question_answers(answers: &str) -> String {
    answers.to_string()  // Pass through as-is
}
```

## Permission Response Format

### HTTP Route (`src/server/routes/permission.rs`)

```rust
#[derive(Deserialize)]
pub struct SubmitPermissionRequest {
    pub session_id: String,
    pub tool: String,
    pub decision: String,  // "allow" or "deny"
    #[serde(default)]
    pub persist: bool,
}

pub async fn submit_permission(
    Path(session_id): Path<String>,
    Json(req): Json<SubmitPermissionRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Validate decision
    if !matches!(req.decision.as_str(), "allow" | "deny") {
        return Err(AppError::Tool(ToolError::Execution(...)));
    }
    // ...
}
```

### WebSocket Message (`src/server/ws.rs`)

```rust
enum TuiMessage {
    PermissionResponse { id: String, choice: String },  // "allow" or "deny"
    // ...
}

// Handling:
TuiMessage::PermissionResponse { id, choice } => {
    let perm_choice = match choice.as_str() {
        "allow" => PermissionChoice::AllowOnce,
        "deny" => PermissionChoice::DenyOnce,
        _ => return,
    };
    let _ = crate::bus::PermissionRegistry::respond(id, perm_choice);
}
```

## Pending State Recovery (Packet 3)

### Server Routes Now Return Real Data

```rust
/// Get pending questions for a session
pub async fn get_pending_questions(Path(session_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    let pending_ids = crate::bus::QuestionRegistry::pending_question_ids();
    let questions: Vec<serde_json::Value> = pending_ids
        .iter()
        .filter(|id| **id == session_id)
        .map(|id| serde_json::json!({"question_id": id, "session_id": id}))
        .collect();
    Ok(Json(serde_json::json!({"questions": questions})))
}

/// Get pending permissions
pub async fn get_pending_permissions(Path(session_id): Path<String>) -> Result<Json<serde_json::Value>, AppError> {
    let pending_ids = crate::bus::PermissionRegistry::pending_permission_ids();
    let permissions: Vec<serde_json::Value> = pending_ids
        .iter()
        .map(|id| {
            let parts: Vec<&str> = id.splitn(2, '-').collect();
            let tool_name = parts.get(1).unwrap_or(&"unknown");
            serde_json::json!({"perm_id": id, "tool": tool_name, "session_id": session_id})
        })
        .collect();
    Ok(Json(serde_json::json!({"permissions": permissions})))
}
```

### Helper Functions for Testing

```rust
/// Helper for testing route behavior without Axum extractors
pub fn get_pending_questions_for_session(session_id: &str) -> serde_json::Value {
    // Same logic as the route handler
}

pub fn get_pending_permissions_for_session(session_id: &str) -> serde_json::Value {
    // Same logic as the route handler
}
```

## Key Changes (Packet 4)

1. **Flexible answer format**: `SubmitQuestionRequest.answers` changed from `Vec<String>` to `serde_json::Value`
2. **WebSocket update**: `QuestionResponse.answers` changed from `Vec<String>` to `serde_json::Value`
3. **Consistent serialization**: Both paths normalize to JSON string before passing to `answer_question()`
4. **Tests updated**: `test_question_tool_answer_immediately` uses object format `{"q1": "blue"}`

## Test Examples

```rust
#[tokio::test]
async fn test_question_answer_format() {
    // Object format
    let answers = serde_json::json!({"q1": "red"}).to_string();
    let answered = QuestionRegistry::answer_question("session-1".to_string(), answers);
    assert!(answered);
    
    // Array format (still supported)
    let answers = serde_json::json!(["answer1"]).to_string();
    let answered = QuestionRegistry::answer_question("session-2".to_string(), answers);
    assert!(answered);
}
```
