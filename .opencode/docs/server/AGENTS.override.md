# Server Module Override

This file contains server-specific guidance and overrides root AGENTS.md.

## Feature Gating
- All server code is feature-gated behind `#[cfg(feature = "server")]`
- Run server-related tests with `cargo test --features server`

## Route Helpers
Extracted helper functions for testability:
- `get_pending_questions_for_session(session_id)` - returns pending question IDs for a session
- `get_pending_permissions_for_session(session_id)` - returns pending permission IDs (tool name parsed from perm_id)

## Question/Permission Response Shape
- `SubmitQuestionRequest.answers` is `serde_json::Value` (supports object and array formats)
- Websocket `QuestionResponse.answers` is `serde_json::Value`
- Both HTTP and WebSocket paths normalize answers to consistent JSON string before passing to registries

## WebSocket Message Handling
- `QuestionResponse` and `PermissionResponse` messages route into respective registries
- Handles both HTTP and WebSocket response paths consistently