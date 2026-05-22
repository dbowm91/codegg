use axum::Json;

pub async fn health_check() -> &'static str {
    "ok"
}
