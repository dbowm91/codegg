use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
    routing::{delete, get, post},
    Router,
};
use http::header;
use tokio::sync::RwLock;
use tower_http::compression::{predicate::Predicate, CompressionLayer};
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use super::middleware::auth::auth_middleware;
use super::routes;
use super::routes::health::health_check;
use super::state::{ServerState, WsRateLimiter};
use super::ws;

#[derive(Clone)]
struct CompressionPredicate;

impl Predicate for CompressionPredicate {
    fn should_compress<B>(&self, res: &http::Response<B>) -> bool
    where
        B: axum::body::HttpBody,
    {
        let status = res.status();
        !matches!(status.as_u16(), 401 | 403 | 404 | 422 | 500 | 502 | 503)
    }
}

#[derive(Clone)]
struct RateLimiter {
    cache: Arc<tokio::sync::Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    async fn check_rate_limit(&self, key: &str) -> (bool, usize) {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        let requests = cache.entry(key.to_string()).or_insert_with(Vec::new);

        requests.retain(|&t| now.duration_since(t) < self.window);

        let remaining = self.max_requests.saturating_sub(requests.len());
        let allowed = requests.len() < self.max_requests;

        if allowed {
            requests.push(now);
        }

        (allowed, remaining)
    }
}

async fn rate_limit_middleware(
    State(rate_limiter): State<RateLimiter>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let key = addr.to_string();

    let (allowed, remaining) = rate_limiter.check_rate_limit(&key).await;
    if !allowed {
        let retry_after = rate_limiter.window.as_secs().to_string();
        return Response::builder()
            .status(429)
            .header("Retry-After", &retry_after)
            .header("X-RateLimit-Limit", &rate_limiter.max_requests.to_string())
            .header("X-RateLimit-Remaining", "0")
            .header("X-RateLimit-Reset", &retry_after)
            .body(Body::from("Too Many Requests"))
            .unwrap_or_else(|_| Response::builder().status(429).body(Body::empty()).unwrap());
    }

    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "X-RateLimit-Limit",
        std::convert::TryFrom::try_from(rate_limiter.max_requests.to_string()).unwrap(),
    );
    response.headers_mut().insert(
        "X-RateLimit-Remaining",
        std::convert::TryFrom::try_from(remaining.to_string()).unwrap(),
    );
    response.headers_mut().insert(
        "X-RateLimit-Reset",
        std::convert::TryFrom::try_from(rate_limiter.window.as_secs().to_string()).unwrap(),
    );

    response
}

fn build_cors(config: &Option<crate::config::schema::ServerConfig>) -> CorsLayer {
    let origins = config
        .as_ref()
        .and_then(|c| c.cors_origins.clone())
        .or_else(|| {
            Some(vec![
                "http://localhost:3000".to_string(),
                "http://127.0.0.1:3000".to_string(),
            ])
        });

    match origins {
        Some(origins) if !origins.is_empty() => {
            let origins: Vec<_> = origins.into_iter().filter_map(|o| o.parse().ok()).collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::DELETE,
                ])
                .allow_headers(tower_http::cors::Any)
        }
        _ => {
            let default_origins = vec![
                "http://localhost:3000".to_string(),
                "http://127.0.0.1:3000".to_string(),
            ];
            let origins: Vec<_> = default_origins
                .into_iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::DELETE,
                ])
                .allow_headers(tower_http::cors::Any)
        }
    }
}

pub async fn run_server(host: &str, port: u16) -> Result<(), crate::error::ServerRuntimeError> {
    let project_dir = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let pool = crate::storage::init(&project_dir)
        .await
        .map_err(|e| crate::error::ServerRuntimeError::Shutdown(e.to_string()))?;

    let config = crate::config::schema::Config::load().ok();
    let server_config = config.as_ref().and_then(|c| c.server.clone());

    let mut mcp_service = crate::mcp::McpService::new();
    if let Some(ref cfg) = config {
        if let Some(ref mcp_entries) = cfg.mcp {
            for (name, entry) in mcp_entries {
                if entry.enabled.unwrap_or(true) {
                    if let Some(ref server_cfg) = entry.inner {
                        let timeout = server_cfg.timeout.unwrap_or(30);
                        let server_type = server_cfg.server_type.as_deref().unwrap_or("local");
                        mcp_service
                            .connect_from_config(
                                name,
                                server_type,
                                server_cfg.command.as_deref(),
                                server_cfg.args.as_deref(),
                                server_cfg
                                    .env
                                    .as_ref()
                                    .or(server_cfg.environment.as_ref())
                                    .cloned(),
                                server_cfg.url.as_deref(),
                                server_cfg.headers.clone(),
                                timeout,
                            )
                            .await
                            .map_err(|e| {
                                tracing::warn!("failed to connect MCP server {}: {}", name, e);
                            })
                            .ok();
                    }
                }
            }
        }
    }

    let state = ServerState {
        project_dir: project_dir.clone(),
        pool,
        mcp_service: Arc::new(RwLock::new(mcp_service)),
        config: config.unwrap_or_default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
    };

    let cors = build_cors(&server_config);

    let rate_limiter = RateLimiter::new(100, 60);

    let compression = CompressionLayer::new()
        .gzip(true)
        .br(true)
        .compress_when(CompressionPredicate);

    let api_router = Router::new()
        .route(
            "/api/sessions",
            get(routes::list_sessions).post(routes::create_session),
        )
        .route("/api/sessions/:id", get(routes::get_session))
        .route("/api/sessions/:id/archive", delete(routes::archive_session))
        .route("/api/sessions/:id/fork", post(routes::fork_session))
        .route("/api/sessions/:id/share", post(routes::share_session))
        .route("/api/sessions/:id/unshare", post(routes::unshare_session))
        .route("/api/sessions/:id/revert", post(routes::revert_session))
        .route("/api/sessions/:id/unrevert", post(routes::unrevert_session))
        .route("/api/sessions/:id/messages", get(routes::list_messages))
        .route("/api/config", get(routes::get_config))
        .route("/api/mcp", get(routes::list_mcp_servers))
        .route("/api/event", get(routes::sse_handler))
        .route(
            "/api/question/:session_id",
            get(routes::get_pending_questions).post(routes::submit_question),
        )
        .route(
            "/api/permission/:session_id",
            get(routes::get_pending_permissions),
        )
        .route(
            "/api/permission/:session_id/submit",
            post(routes::submit_permission),
        )
        .route("/api/providers", get(routes::list_providers))
        .route("/api/tools", get(routes::list_tools))
        .route("/api/file/read", get(routes::read_file))
        .route("/api/file/list", get(routes::list_files))
        .route("/api/file/write", post(routes::write_file))
        .route("/api/file/delete", delete(routes::delete_file))
        .route(
            "/api/project",
            get(routes::get_project).post(routes::create_project),
        )
        .route("/api/project/list", get(routes::list_projects))
        .route(
            "/api/workspace",
            get(routes::get_workspace).post(routes::create_workspace),
        )
        .route("/api/workspace/list", get(routes::list_workspaces))
        .route("/ws", get(ws::handle_ws))
        .route("/tui", get(ws::handle_tui))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter,
            rate_limit_middleware,
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(cors)
        .layer(compression)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api", api_router);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::error::ServerRuntimeError::Bind(e.to_string()))?;

    info!("Server listening on {}", addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| crate::error::ServerRuntimeError::Shutdown(e.to_string()))?;

    Ok(())
}
